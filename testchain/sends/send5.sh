#!/bin/bash
for ((i=1;i<500;i++)); do  echo $i; 
time curl --data '{"method":"eth_sendTransaction","params":[{"from":"0x0063ec1c2b77e2d1f9cd937e2b158a988e3f77c0","to":"0x005b0fbe9a9a53e66aca408e9dc2f9c53cbd6665","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}' -H "Content-Type: application/json" -X POST localhost:8676; 
done