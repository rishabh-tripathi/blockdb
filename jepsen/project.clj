(defproject jepsen.blockdb "0.1.0-SNAPSHOT"
  :description "Jepsen test for BlockDB: durability/consistency under partitions."
  :url "https://github.com/rishabh-tripathi/blockdb"
  :license {:name "MIT"}
  :main jepsen.blockdb
  :dependencies [[org.clojure/clojure "1.11.1"]
                 [jepsen "0.3.5"]
                 [clj-http "3.12.3"]
                 [cheshire "5.11.0"]]
  :jvm-opts ["-Djava.awt.headless=true"])
