#!/bin/bash
for ((i=1;i<500;i++)); do  echo $i; 
time curl --data '{"method":"eth_sendTransaction","params":[{"from":"0x002d7b6716b90ef6a10c9ecbf4bf1056cd62a41c","to":"0x0049555fbcd81a300481f8bab352f2bd0679140e","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}' -H "Content-Type: application/json" -X POST localhost:8669; 
sleep 2;
done