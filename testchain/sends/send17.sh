#!/bin/bash
for ((i=1;i<500;i++)); do  echo $i; 
time curl --data '{"method":"eth_sendTransaction","params":[{"from":"0x00c3ca2fd819f4d2ea30c9fd99bf80c7c86f1f25","to":"0x002db24c08ed9397bc77a554e55f80d56be7b15f","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}' -H "Content-Type: application/json" -X POST localhost:8662; 
sleep 1;
done