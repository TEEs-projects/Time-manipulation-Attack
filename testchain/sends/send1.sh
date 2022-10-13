#!/bin/bash
for ((i=1;i<500;i++)); do  echo $i; 
time curl --data '{"method":"eth_sendTransaction","params":[{"from":"0x00e46a5a194748871d4d17ac88d657f63b1c50e3","to":"0x0054076b6784fc25baf961db2ebc760a49a14379","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}' -H "Content-Type: application/json" -X POST localhost:8672; 
done