#!/bin/bash
for ((i=1;i<500;i++)); do  echo $i; 
time curl --data '{"method":"eth_sendTransaction","params":[{"from":"0x004f49d9267bce6bdefc0fe9065269fa5d24ead9","to":"0x004edc8b40e4c8210e7c25cd9236f2461bbf1ada","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}' -H "Content-Type: application/json" -X POST localhost:8665; 
sleep 1;
done