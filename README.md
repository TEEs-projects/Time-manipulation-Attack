# Timestamp-attack-of-Aura

This experimental implementation, based on the Rust programming language, explores the time-manipulation attack proposed in the paper 'Breaking Fairness against Proof of Authority Aura', which will be presented at the 2023 ACM Web Conference.

## Table of Contents

1. [Description](#chapter-001)<br>
2. [Getting Started](#chapter-001)<br>
  2.1 [Prerequisites](#chapter-0011)<br>
  2.2 [Build](#chapter-0012)<br>
3. [Running the tests](#chapter-002)<br>
  3.1 [Step by step](#chapter-0021)<br>
  3.2 [Analysis tools](#chapter-0022)<br>
4. [Experimental results](#chapter-003)<br>
5. [Acknowledgments](#chapter-004)<br>


## **1 Description**<a id="chapter-000"></a>
**Built for research use**: a novel attack series on OpenEthereum, an implementation of Proof-of-Authority Aura.

**Built for research use**: This tool is designed for research purposes and is capable of conducting a novel series of attacks on OpenEthereum, an implementation of Proof-of-Authority Aura.

* It enables complete reproducibility of the attacks presented in the research paper and includes source code with historical data. 
* The tool consists of three distinct attacks and offers a local test environment for ease of use.


## **2 Getting Started**<a id="chapter-001"></a>

The codes are developed in Rust and Python (for data processing tools) using a server with Ubuntu 20.04.4 LTS (GNU/Linux 5.4.0-109-generic x86\_64) operating system.

### **2.1 Prerequisites**<a id="chapter-0011"></a>

* [Rust](https://www.rust-lang.org/)
* By default, websocket 8650-8675 and 8750-8775 need to be avaliable (considering the scenarios of running 21 sealer nodes and 5 user nodes).

### **2.2 Build**<a id="chapter-0012"></a>


To build the project, enter each *openethereum* directory and run:

```
cargo build
```

## **3 Running the tests**<a id="chapter-002"></a>
<img src=./testchain/pic.png width=60% />

We establish 21 sealer nodes (node0 to node20) and 5 user nodes (usr1 to usr5) for testing. Among them, sealer0 is a malicious sealer running a falsified client (either openethereum-3.3.4_sleep3s, openethereum-3.3.4_25s or openethereum-3.3.4_23s_sleep3s). The specific falsified client can be selected by changing the directory on line 2 of [file](https://github.com/TEEs-projects/Time-manipulation-Attack/blob/main/testchain/nohuprun.sh). [Openethereum](https://github.com/TEEs-projects/Time-manipulation-Attack/tree/main/openethereum) is the original client run by honest sealers/users.

### **3.1 Step by step**<a id="chapter-0021"></a>

* To run the test, enter *./testchain* directory and run:

```
./nohuprun.sh
```
21 sealer nodes and 5 user nodes start running on local machine, which is editable in file *./nohuprun.sh* along with attacking methods of malicious node0.

* To enable the Python auxiliary tool for current blockchain state, run:
```
./start.sh
```
Follow the instruction and type in the span of blocks to be analyzed, and results will be outputted as

1. logs/.
2. qry_result.txt
3. result_readable.txt
4. results_indexes.txt


* To send transactions to the system, make sure usr1 to usr5 are running and run:
```
./send.sh
```

* To stop the running, run:
```
./stop.sh
```

* After the stop, to clean up the current chain data and operation results, run:
```
./clean.sh
```

### **3.2 Analysis tools**<a id="chapter-0022"></a>

**Python auxiliary tools for data processing** can be found in directory *./testchain/shellgen*. Tools are built for analytical purposes including data query, cutting, counting, and outputting analysis results to files that are easy to read.


## **4 Experimental results**<a id="chapter-003"></a>

Historical data of our runs are in [*results*](https://github.com/auraAttack/Time-manipulation-Attack/tree/main/results) folder, which reflects our experimental results as described in Section 4.2 of our paper.

Once the attack starts, all node1's blocks are lost for all [*Attack-II*](https://github.com/TEEs-projects/Time-manipulation-Attack/blob/main/results/25s/result_indexes.txt), [*Attack-II*](https://github.com/TEEs-projects/Time-manipulation-Attack/blob/main/results/sleep3s/result_indexes.txt) and [*Attack-III*](https://github.com/TEEs-projects/Time-manipulation-Attack/tree/main/results). Details can be found [here](https://github.com/TEEs-projects/Time-manipulation-Attack/tree/main/results).



## 5 Acknowledgments<a id="chapter-004"></a>
Attacking source code is developed based on:
* [Openethereum 3.3.4](https://github.com/openethereum/openethereum/tree/v3.3.4)

