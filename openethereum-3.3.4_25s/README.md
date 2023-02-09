## Attacking Method-I: Timestamp Falsify

### **Build**


To build the project, run:

```
cargo build
```

### **Run**
Follow the section **Running the tests** in [instruction](https://github.com/auraAttack/Time-manipulation-Attack).

### **Attacking point**
By maliciously setting the timestamp [here](https://github.com/TEEs-projects/Time-manipulation-Attack/blob/fc3d0e58baa1a6b4168ce652811d2381b53badd2/openethereum-3.3.4_25s/crates/ethcore/src/engines/mod.rs#L537), attackers call the alternative function [here](https://github.com/TEEs-projects/Time-manipulation-Attack/blob/fc3d0e58baa1a6b4168ce652811d2381b53badd2/openethereum-3.3.4_25s/crates/ethcore/src/block.rs#L213) to falsify the timestamp of its newly produced block whenever [in turn](https://github.com/TEEs-projects/Time-manipulation-Attack/blob/fc3d0e58baa1a6b4168ce652811d2381b53badd2/openethereum-3.3.4_25s/crates/ethcore/src/block.rs#L209).
