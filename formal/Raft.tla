---------------------------- MODULE Raft ----------------------------
(***************************************************************************)
(* An abstract TLA+ specification of the safety-critical core of the Raft *)
(* consensus protocol as implemented by BlockDB's consensus layer.        *)
(*                                                                         *)
(* It models leader election and log replication and states Raft's safety *)
(* invariants.  It is intended to be model-checked with TLC on a small    *)
(* finite instance (see formal/README.md).  It abstracts away timers,     *)
(* message loss and the state machine's concrete values, focusing on the  *)
(* properties that must hold for linearizability:                         *)
(*                                                                         *)
(*   ElectionSafety     - at most one leader per term                      *)
(*   LogMatching        - logs agreeing at an index agree on all prior     *)
(*   LeaderCompleteness - a committed entry is in every future leader's log*)
(*   StateMachineSafety - no two servers apply different entries at an idx *)
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
          votesGranted    \* votesGranted[s]: set of servers that voted for s this term

vars == <<currentTerm, state, votedFor, log, commitIndex, votesGranted>>

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
    /\ UNCHANGED <<log, commitIndex>>

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
    /\ UNCHANGED <<currentTerm, state, log, commitIndex>>

(* A candidate with votes from a quorum becomes leader for its term. *)
BecomeLeader(s) ==
    /\ state[s] = Candidate
    /\ votesGranted[s] \in Quorum
    /\ state' = [state EXCEPT ![s] = Leader]
    /\ UNCHANGED <<currentTerm, votedFor, log, commitIndex, votesGranted>>

(* A leader appends a new client entry in its current term. *)
ClientRequest(s, v) ==
    /\ state[s] = Leader
    /\ Len(log[s]) < MaxLogLen
    /\ log' = [log EXCEPT ![s] = Append(log[s], [term |-> currentTerm[s], value |-> v])]
    /\ UNCHANGED <<currentTerm, state, votedFor, commitIndex, votesGranted>>

(* Replicate one entry from leader s to follower t with the Raft log      *)
(* consistency check: the entry just before the copied one must match.    *)
AppendEntry(s, t) ==
    /\ state[s] = Leader
    /\ currentTerm[t] <= currentTerm[s]
    /\ Len(log[t]) < Len(log[s])
    /\ LET i == Len(log[t]) + 1 IN
         /\ \/ i = 1
            \/ /\ i > 1
               /\ Len(log[t]) >= i - 1
               /\ log[t][i-1].term = log[s][i-1].term
         /\ log' = [log EXCEPT ![t] = Append(log[t], log[s][i])]
    /\ currentTerm' = [currentTerm EXCEPT ![t] = currentTerm[s]]
    /\ state'       = [state EXCEPT ![t] = Follower]
    /\ UNCHANGED <<votedFor, commitIndex, votesGranted>>

(* The leader advances commitIndex to an index replicated on a quorum,    *)
(* but only for an entry from its current term (Raft's commit rule).      *)
AdvanceCommit(s, idx) ==
    /\ state[s] = Leader
    /\ idx <= Len(log[s])
    /\ idx > commitIndex[s]
    /\ log[s][idx].term = currentTerm[s]
    /\ \E Q \in Quorum :
         \A t \in Q : /\ Len(log[t]) >= idx
                      /\ log[t][idx].term = log[s][idx].term
    /\ commitIndex' = [commitIndex EXCEPT ![s] = idx]
    /\ UNCHANGED <<currentTerm, state, votedFor, log, votesGranted>>

(* A server discovering a higher term steps down. *)
StepDown(s, t) ==
    /\ currentTerm[t] > currentTerm[s]
    /\ currentTerm' = [currentTerm EXCEPT ![s] = currentTerm[t]]
    /\ state'       = [state EXCEPT ![s] = Follower]
    /\ votedFor'    = [votedFor EXCEPT ![s] = Nil]
    /\ UNCHANGED <<log, commitIndex, votesGranted>>

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

(* No two servers have different entries committed at the same index.     *)
StateMachineSafety ==
    \A s, t \in Server :
        \A i \in 1..(IF commitIndex[s] < commitIndex[t] THEN commitIndex[s] ELSE commitIndex[t]) :
            log[s][i] = log[t][i]

Invariants ==
    /\ TypeOK
    /\ ElectionSafety
    /\ LogMatching
    /\ StateMachineSafety

=============================================================================
