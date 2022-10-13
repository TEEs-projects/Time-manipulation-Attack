# coding=utf-8
file = open('/data/xr/testchain/qry.sh','w')
filetx = open('/data/xr/testchain/txqry.sh','w')
filetx_out1=open('/data/xr/testchain/tx_result.txt','w')
p1='curl --data \'{"method":"eth_getBlockByNumber","params":["'
p2='",true],"id":1,"jsonrpc":"2.0"}\' -H "Content-Type: application/json" -X POST localhost:8651 >> qry_result.txt \n\n'
p3='"/\/n/\/n/\/n" >> qry_result.txt \n'
tx1='curl --data \'{"method":"eth_getBlockTransactionCountByNumber","params":["'
tx2='"],"id":1,"jsonrpc":"2.0"}\' -H "Content-Type: application/json" -X POST localhost:8652 >> tx_result.txt \n\n'
do_cut = "python3 ./shellgen/cut_result.py"
do_cut_tx = "python3 ./shellgen/cut_tx.py"
floor = input("input lower bound # = ")
ceiling = input("input upper bound # = ")
floor = int(floor)
ceiling = int(ceiling)
filetx_out1.write(str(floor)+'\n'+str(ceiling)+'\n')
print("querying info for #"+str(floor)+" to #"+str(ceiling)+"\n")
for i in range (floor,ceiling):
    n=str(hex(i))
    file.write(p1+n+p2)
    filetx.write(tx1+n+tx2)
file.write(do_cut)
filetx.write(do_cut_tx)
file.close()
filetx.close()
