## **Data processing tools**
This folder holds the chain and node configuration files for running our experiment.

### **Catalog**
Following files are kept in this folder:

* **genqry.py**: Generate a batched block information query file *qry.sh* in *testchain*, along with *txqry.sh* and *tx_result.txt* for transaction data processing. 
* **cut_result.py**: *qry_result.txt* is generated after executing *qry.sh*. Then *cut_result.py* cuts the *qry_result.txt* and compose it into reading friendly files *result_readable.txt* and *result_indexes.txt* in directory *testchain*.
* **cut_tx.py**: Cut the *tx_result.txt* and compose it into reading friendly files *tx_read.txt*.

Above files are automatically executed when *./start.sh* is called during runtime.