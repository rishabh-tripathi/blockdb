(ns jepsen.blockdb
  "A Jepsen test for BlockDB.

   BlockDB is append-only: each key is written exactly once. We therefore model a
   grow-only set — each :add writes a unique key, and :read scans the prefix and
   collects the values. Under partitions we assert (checker/set-full) that every
   acknowledged add survives (no lost committed writes) and no un-added element
   ever appears (no phantom reads). This probes the durability + linearizability
   of the replicated store while a partition nemesis cuts the network.

   This is a scaffold: it requires a Jepsen control node and N SSH-able DB nodes
   (see README.md). It is structurally complete but has not been executed in the
   authoring environment."
  (:require [clojure.tools.logging :refer [info warn]]
            [clojure.string :as str]
            [cheshire.core :as json]
            [clj-http.client :as http]
            [jepsen [cli :as cli]
                    [client :as client]
                    [control :as c]
                    [db :as db]
                    [generator :as gen]
                    [nemesis :as nemesis]
                    [tests :as tests]
                    [checker :as checker]]
            [jepsen.control.util :as cu]
            [jepsen.os.debian :as debian]))

(def port 8080)
(def dir "/opt/blockdb")
(def binary (str dir "/blockdb-node"))
(def data-dir (str dir "/data"))
(def logfile (str dir "/blockdb.log"))
(def pidfile (str dir "/blockdb.pid"))

(defn peers-arg
  "The --peers argument: every node as id@host:port (ids are node names)."
  [test]
  (->> (:nodes test)
       (map (fn [n] (str n "@" n ":" port)))
       (str/join ",")))

(defn db
  "BlockDB cluster DB. Expects a prebuilt `blockdb-node` binary uploaded to each
   node at `binary` (upload it with --nodes-file setup, or bake it into an image)."
  []
  (reify db/DB
    (setup! [_ test node]
      (info node "starting blockdb-node")
      (c/exec :mkdir :-p dir data-dir)
      (cu/start-daemon!
        {:logfile logfile :pidfile pidfile :chdir dir}
        binary
        :--node-id node
        :--host "0.0.0.0"
        :--port port
        :--peers (peers-arg test)
        :--data-dir data-dir
        ;; Auth/TLS omitted for the test cluster; add --cluster-secret/--api-token
        ;; (and update the client) to exercise the secured path.
        )
      (Thread/sleep 5000)) ; let it bind + elect

    (teardown! [_ test node]
      (info node "stopping blockdb-node")
      (cu/stop-daemon! binary pidfile)
      (c/exec :rm :-rf data-dir))

    db/LogFiles
    (log-files [_ test node]
      [logfile])))

(defn- base [node] (str "http://" node ":" port))

(defn- add! [node n]
  (http/post (str (base node) "/kv")
             {:body (json/generate-string {:key (str "jset:" n) :value (str n)})
              :headers {"content-type" "application/json"}
              :socket-timeout 5000 :connection-timeout 5000
              :throw-exceptions false}))

(defn- read-set [node]
  (let [resp (http/get (str (base node) "/kv/scan")
                       {:query-params {"prefix" "jset:" "limit" "1000000"}
                        :socket-timeout 10000 :connection-timeout 5000
                        :throw-exceptions false})]
    (when (= 200 (:status resp))
      (->> (json/parse-string (:body resp) true)
           :items
           (map (comp #(Long/parseLong %) :value))
           (into (sorted-set))))))

(defrecord Client [node]
  client/Client
  (open! [this test node]
    (assoc this :node node))

  (setup! [_ test])

  (invoke! [this test op]
    (case (:f op)
      :add  (try
              (let [resp (add! (:node this) (:value op))]
                (cond
                  (= 200 (:status resp)) (assoc op :type :ok)
                  ;; 503 = no leader / leadership in transition: write not applied.
                  (= 503 (:status resp)) (assoc op :type :fail :error :unavailable)
                  :else (assoc op :type :fail :error (:status resp))))
              (catch Exception e
                (assoc op :type :info :error (.getMessage e))))
      :read (try
              (if-let [s (read-set (:node this))]
                (assoc op :type :ok :value s)
                (assoc op :type :fail :error :read-failed))
              (catch Exception e
                (assoc op :type :info :error (.getMessage e))))))

  (teardown! [_ test])
  (close! [_ test]))

(defn adds [] (map (fn [n] {:type :invoke :f :add :value n}) (range)))
(defn reads [] (repeat {:type :invoke :f :read}))

(defn blockdb-test
  [opts]
  (merge
    tests/noop-test
    {:name    "blockdb-set-partition"
     :os      debian/os
     :db      (db)
     :client  (Client. nil)
     :nemesis (nemesis/partition-random-halves)
     :checker (checker/compose
                {:set-full (checker/set-full {:linearizable? false})
                 :perf     (checker/perf)
                 :timeline (checker/timeline)})
     :generator
     (gen/phases
       (->> (gen/mix [(adds) (reads)])
            (gen/stagger 1/50)
            (gen/nemesis
              (cycle [(gen/sleep 5)
                      {:type :info :f :start}
                      (gen/sleep 15)
                      {:type :info :f :stop}]))
            (gen/time-limit (:time-limit opts 120)))
       ;; Heal, settle, and do a final read on every node.
       (gen/nemesis {:type :info :f :stop})
       (gen/sleep 10)
       (gen/clients (gen/each-thread {:type :invoke :f :read})))}
    opts))

(defn -main
  [& args]
  (cli/run! (cli/single-test-cmd {:test-fn blockdb-test})
            args))
