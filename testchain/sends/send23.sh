#!/bin/bash
for ((i=1;i<500;i++)); do  echo $i; 
time curl --data '{"method":"eth_sendTransaction","params":[{"from":"0x00a6a2655ad6707e925bb6949a933f05690288bb","to":"0x002d7b6716b90ef6a10c9ecbf4bf1056cd62a41c","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}' -H "Content-Type: application/json" -X POST localhost:8668; 
sleep 2;
done