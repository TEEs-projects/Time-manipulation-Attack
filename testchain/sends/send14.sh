#!/bin/bash
for ((i=1;i<500;i++)); do  echo $i; 
time curl --data '{"method":"eth_sendTransaction","params":[{"from":"0x001ce0f63558e2fe10806d132d64d2b2f63ef64e","to":"0x00aa39d30f0d20ff03a22ccfc30b7efbfca597c2","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}' -H "Content-Type: application/json" -X POST localhost:8659; 
done