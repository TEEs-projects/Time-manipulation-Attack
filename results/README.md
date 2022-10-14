## **Historical data**
This folder holds our historical experimental data, supporting the validity of our statement in paper **Time-manipulation Attack: Breaking Fairness against Proof of Authority Aura** 

### **Catalog**
Data are classified by three attacking methods:

* **Timestamp Falsify**: Folder *25s*
* **Sleep Delay**: Folder *sleep2s, sleep3s*
* **Timestamp Falsify & Sleep Delay**: Folder *23s_sleep3s*

Each above directory contains running logs of all $21$ sealer nodes under directories **/logs* and processed data by our Python auxiliary tools, including *result_indexes.txt and result_readable.txt*.

Folder **txs** keeps data in experiments where transactions are involed, and is catagorized as the same way above.

