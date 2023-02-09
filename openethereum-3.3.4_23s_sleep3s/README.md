## Attacking Method-III: Timestamp Falsify & Sleep Delay
### **Build**


To build the project, run:

```
cargo build
```

### **Run**
Follow the section **Running the tests** in [instruction](https://github.com/auraAttack/Time-manipulation-Attack).

### **Attacking point**
Attackers [set](https://github.com/TEEs-projects/Time-manipulation-Attack/blob/db90d706e4953b6a8980bccc110d3240819ff478/openethereum-3.3.4_23s_sleep3s/crates/ethcore/src/block.rs#L213) the timestamps of their blocks by calling the pre-designed malicious [function](https://github.com/TEEs-projects/Time-manipulation-Attack/blob/e19cf1d858fe1eb3e87b632347f2102d6cb0c4c0/openethereum-3.3.4_23s_sleep3s/crates/ethcore/src/engines/mod.rs#L517), then [sleep](https://github.com/TEEs-projects/Time-manipulation-Attack/blob/db90d706e4953b6a8980bccc110d3240819ff478/openethereum-3.3.4_23s_sleep3s/crates/ethcore/src/block.rs#L215) for 3 seconds immediately.
