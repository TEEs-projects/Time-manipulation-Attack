#!/bin/bash
for ((i=1;i<500;i++)); do  echo $i; 
time curl --data '{"method":"eth_sendTransaction","params":[{"from":"0x0054076b6784fc25baf961db2ebc760a49a14379","to":"0x005b0fbe9a9a53e66aca408e9dc2f9c53cbd6665","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}' -H "Content-Type: application/json" -X POST localhost:8674; 
done