#!/bin/bash
for ((i=1;i<500;i++)); do  echo $i; 
time curl --data '{"method":"eth_sendTransaction","params":[{"from":"0x005b0fbe9a9a53e66aca408e9dc2f9c53cbd6665","to":"0x00379d1ae3b1def5241a44369397a4dadb1dff64","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}' -H "Content-Type: application/json" -X POST localhost:8671; 
done