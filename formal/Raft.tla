---------------------------- MODULE Raft ----------------------------
(***************************************************************************)
(* An abstract TLA+ specification of the safety-critical core of the Raft  *)
(* consensus protocol as implemented by BlockDB's consensus layer.         *)
(*                                                                         *)
(* It models leader election and log replication — including conflicting-  *)
(* entry truncation (Raft 5.3) — and states Raft's safety invariants.      *)
(* It is intended to be model-checked with TLC on a small finite instance  *)
(* (see formal/README.md and MC_Raft.cfg). It abstracts away timers and    *)
(* message loss, focusing on the properties that must hold for             *)
(* linearizability:                                                        *)
(*                                                                         *)
(*   ElectionSafety     - at most one leader per term                      *)
(*   LogMatching        - logs agreeing at an index agree on all prior     *)
(*   LeaderCompleteness - a committed entry is present in the log of every *)
(*                        leader of a term >= the entry's commit term      *)
(*   StateMachineSafety - no two servers apply different entries at an idx *)
(*   CommitIndexBounded - commitIndex never exceeds the log length         *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS Server          \* set of server ids, e.g. {s1, s2, s3}
CONSTANTS Value           \* set of client values, e.g. {v1, v2}
CONSTANTS MaxTerm         \* bound on terms, for finite checking
CONSTANTS MaxLogLen       \* bound on log length, for finite checking

Follower  == "follower"
Candidate == "candidate"
Leader    == "leader"

VARIABLES currentTerm,    \* currentTerm[s]: term of server s
          state,          \* state[s]: follower/candidate/leader
          votedFor,       \* votedFor[s]: candidate voted for in currentTerm, or Nil
          log,            \* log[s]: sequence of [term |-> t, value |-> v]
          commitIndex,    \* commitIndex[s]: highest index known committed
          votesGranted,   \* votesGranted[s]: set of servers that voted for s this term
          committed       \* history: set of [index, term, value] entries ever committed

vars == <<currentTerm, state, votedFor, log, commitIndex, votesGranted, committed>>

Nil == "nil"

Quorum == {Q \in SUBSET Server : Cardinality(Q) * 2 > Cardinality(Server)}

LastTerm(l) == IF Len(l) = 0 THEN 0 ELSE l[Len(l)].term

----------------------------------------------------------------------
(* Initial state: everyone a follower at term 0 with an empty log. *)
Init ==
    /\ currentTerm = [s \in Server |-> 0]
    /\ state       = [s \in Server |-> Follower]
    /\ votedFor    = [s \in Server |-> Nil]
    /\ log         = [s \in Server |-> << >>]
    /\ commitIndex = [s \in Server |-> 0]
    /\ votesGranted = [s \in Server |-> {}]
    /\ committed   = {}

----------------------------------------------------------------------
(* A server times out, bumps its term, and stands for election,        *)
(* voting for itself.                                                   *)
Timeout(s) ==
    /\ currentTerm[s] < MaxTerm
    /\ state[s] \in {Follower, Candidate}
    /\ currentTerm' = [currentTerm EXCEPT ![s] = currentTerm[s] + 1]
    /\ state'       = [state EXCEPT ![s] = Candidate]
    /\ votedFor'    = [votedFor EXCEPT ![s] = s]
    /\ votesGranted' = [votesGranted EXCEPT ![s] = {s}]
    /\ UNCHANGED <<log, commitIndex, committed>>

(* Candidate s requests a vote from t and t grants it, iff t has not     *)
(* voted this term and s's log is at least as up-to-date as t's.         *)
RequestVote(s, t) ==
    /\ state[s] = Candidate
    /\ currentTerm[s] = currentTerm[t]
    /\ votedFor[t] \in {Nil, s}
    /\ \/ LastTerm(log[s]) > LastTerm(log[t])
       \/ /\ LastTerm(log[s]) = LastTerm(log[t])
          /\ Len(log[s]) >= Len(log[t])
    /\ votedFor'     = [votedFor EXCEPT ![t] = s]
    /\ votesGranted' = [votesGranted EXCEPT ![s] = votesGranted[s] \cup {t}]
    /\ UNCHANGED <<currentTerm, state, log, commitIndex, committed>>

(* A candidate with votes from a quorum becomes leader for its term. *)
BecomeLeader(s) ==
    /\ state[s] = Candidate
    /\ votesGranted[s] \in Quorum
    /\ state' = [state EXCEPT ![s] = Leader]
    /\ UNCHANGED <<currentTerm, votedFor, log, commitIndex, votesGranted, committed>>

(* A leader appends a new client entry in its current term. *)
ClientRequest(s, v) ==
    /\ state[s] = Leader
    /\ Len(log[s]) < MaxLogLen
    /\ log' = [log EXCEPT ![s] = Append(log[s], [term |-> currentTerm[s], value |-> v])]
    /\ UNCHANGED <<currentTerm, state, votedFor, commitIndex, votesGranted, committed>>

(* Replicate from leader s to follower t one entry at index i, with the   *)
(* Raft consistency check on the preceding entry. If t holds a conflicting *)
(* entry at i, it is truncated and replaced (Raft 5.3). A follower never   *)
(* overwrites its own committed prefix (i > commitIndex[t]).               *)
AppendEntry(s, t) ==
    /\ state[s] = Leader
    /\ currentTerm[t] <= currentTerm[s]
    /\ \E i \in 1..Len(log[s]) :
         /\ i <= Len(log[t]) + 1
         /\ i > commitIndex[t]
         /\ \/ i = 1
            \/ /\ i - 1 <= Len(log[t])
               /\ log[t][i-1].term = log[s][i-1].term
         \* Only act when this actually changes t's log (append or fix conflict).
         /\ \/ i > Len(log[t])
            \/ log[t][i] /= log[s][i]
         /\ log' = [log EXCEPT ![t] = Append(SubSeq(log[t], 1, i-1), log[s][i])]
    /\ currentTerm' = [currentTerm EXCEPT ![t] = currentTerm[s]]
    /\ state'       = [state EXCEPT ![t] = Follower]
    /\ UNCHANGED <<votedFor, commitIndex, votesGranted, committed>>

(* The leader advances commitIndex to an index replicated on a quorum, but *)
(* only for an entry from its current term (Raft's commit rule). The newly *)
(* committed entry is recorded in the `committed` history with its commit  *)
(* term (= the entry's term), for the LeaderCompleteness invariant.        *)
AdvanceCommit(s, idx) ==
    /\ state[s] = Leader
    /\ idx <= Len(log[s])
    /\ idx > commitIndex[s]
    /\ log[s][idx].term = currentTerm[s]
    /\ \E Q \in Quorum :
         \A t \in Q : /\ Len(log[t]) >= idx
                      /\ log[t][idx].term = log[s][idx].term
    /\ commitIndex' = [commitIndex EXCEPT ![s] = idx]
    /\ committed'   = committed \cup
         {[index |-> idx, term |-> log[s][idx].term, value |-> log[s][idx].value]}
    /\ UNCHANGED <<currentTerm, state, votedFor, log, votesGranted>>

(* A server discovering a higher term steps down. *)
StepDown(s, t) ==
    /\ currentTerm[t] > currentTerm[s]
    /\ currentTerm' = [currentTerm EXCEPT ![s] = currentTerm[t]]
    /\ state'       = [state EXCEPT ![s] = Follower]
    /\ votedFor'    = [votedFor EXCEPT ![s] = Nil]
    /\ UNCHANGED <<log, commitIndex, votesGranted, committed>>

Next ==
    \/ \E s \in Server : Timeout(s)
    \/ \E s, t \in Server : RequestVote(s, t)
    \/ \E s \in Server : BecomeLeader(s)
    \/ \E s \in Server, v \in Value : ClientRequest(s, v)
    \/ \E s, t \in Server : AppendEntry(s, t)
    \/ \E s \in Server, idx \in 1..MaxLogLen : AdvanceCommit(s, idx)
    \/ \E s, t \in Server : StepDown(s, t)

Spec == Init /\ [][Next]_vars

----------------------------------------------------------------------
(* Safety invariants. *)

TypeOK ==
    /\ currentTerm \in [Server -> 0..MaxTerm]
    /\ state \in [Server -> {Follower, Candidate, Leader}]
    /\ commitIndex \in [Server -> 0..MaxLogLen]

(* commitIndex never points past the end of the log. *)
CommitIndexBounded ==
    \A s \in Server : commitIndex[s] <= Len(log[s])

(* At most one leader per term. *)
ElectionSafety ==
    \A s, t \in Server :
        (state[s] = Leader /\ state[t] = Leader /\ currentTerm[s] = currentTerm[t])
            => s = t

(* If two logs contain an entry with the same index and term, the logs    *)
(* are identical up to that index.                                        *)
LogMatching ==
    \A s, t \in Server :
        \A i \in 1..Len(log[s]) :
            (i <= Len(log[t]) /\ log[s][i].term = log[t][i].term)
                => \A j \in 1..i : log[s][j] = log[t][j]

(* Leader Completeness: any committed entry is present, unchanged, in the  *)
(* log of every leader whose term is at least the entry's commit term.     *)
LeaderCompleteness ==
    \A s \in Server :
        state[s] = Leader =>
            \A c \in committed :
                (currentTerm[s] >= c.term) =>
                    /\ c.index <= Len(log[s])
                    /\ log[s][c.index].term  = c.term
                    /\ log[s][c.index].value = c.value

(* No two servers have different entries committed at the same index.     *)
StateMachineSafety ==
    \A s, t \in Server :
        \A i \in 1..(IF commitIndex[s] < commitIndex[t] THEN commitIndex[s] ELSE commitIndex[t]) :
            log[s][i] = log[t][i]

Invariants ==
    /\ TypeOK
    /\ CommitIndexBounded
    /\ ElectionSafety
    /\ LogMatching
    /\ LeaderCompleteness
    /\ StateMachineSafety

=============================================================================
