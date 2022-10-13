#!/bin/bash
for ((i=1;i<500;i++)); do  echo $i; 
time curl --data '{"method":"eth_sendTransaction","params":[{"from":"0x008272a8cfd2d3d0f3edc823b1bb729cb73f09db","to":"0x00bd138abd70e2f00903268f3db08f2d25677c9e","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}' -H "Content-Type: application/json" -X POST localhost:8658; 
done