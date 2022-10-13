#!/bin/bash
for ((i=1;i<500;i++)); do  echo $i; 
time curl --data '{"method":"eth_sendTransaction","params":[{"from":"0x003f3b1f635b2dd9a4518c33098e5f72214d6a1e","to":"0x001ce0f63558e2fe10806d132d64d2b2f63ef64e","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}' -H "Content-Type: application/json" -X POST localhost:8657; 
done