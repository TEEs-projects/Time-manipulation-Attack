# coding=utf-8
file_in_tx = open('/data/xr/testchain/tx_result.txt','r')
file_out = open('/data/xr/testchain/tx_read.txt','w')

tx_content = file_in_tx.read()
tx_list = tx_content.split('\n')

floor = int(tx_list[0])
ceiling = int(tx_list[1])
n = floor
total = 0

def write_tx(line,n):
	r = line
	idx = n
	txs_hex = r[r.find("result") + 9:r.find("id") - 3] 
	txs = int(txs_hex,16)
	p = '#'+str(idx)+'\t'+str(txs)+'\n'
	n = n+1
	file_out.write(p)
	return txs


for i in range(2,len(tx_list)-1):
	txs = write_tx(tx_list[i],n)
	total = total+txs
	n=n+1

file_out.write('total txs = '+str(total)+'\n')
