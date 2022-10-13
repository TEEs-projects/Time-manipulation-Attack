#!/bin/bash
for ((i=1;i<500;i++)); do  echo $i; 
time curl --data '{"method":"eth_sendTransaction","params":[{"from":"0x0032d84dff7be846333990d48d05db2a670089ad","to":"0x00e46a5a194748871d4d17ac88d657f63b1c50e3","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}' -H "Content-Type: application/json" -X POST localhost:8675; 
done