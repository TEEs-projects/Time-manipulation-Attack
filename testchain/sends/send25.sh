#!/bin/bash
for ((i=1;i<500;i++)); do  echo $i; 
time curl --data '{"method":"eth_sendTransaction","params":[{"from":"0x0049555fbcd81a300481f8bab352f2bd0679140e","to":"0x00bd138abd70e2f00903268f3db08f2d25677c9e","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}' -H "Content-Type: application/json" -X POST localhost:8670; 
sleep 2;
done