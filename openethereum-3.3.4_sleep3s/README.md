## Attacking Method-II: Sleep Delay

### **Build**


To build the project, run:

```
cargo build
```

### **Run**
Follow the section **Running the tests** in [instruction](https://github.com/auraAttack/Time-manipulation-Attack).

### **Attacking point**
The attackers [sleep](https://github.com/TEEs-projects/Time-manipulation-Attack/blob/80e4ff718f0718e8e21c491d0fbf4c02d58c8215/openethereum-3.3.4_sleep3s/crates/ethcore/src/block.rs#L211) for 3 seconds everytime they are in turn to produce a block. 
