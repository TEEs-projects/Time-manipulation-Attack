# coding=utf-8
# Step1: make dir for 21 nodes
def mkdir():
    file = open('./mkdir.sh','w')
    p1 = 'mkdir ./node'
    for i in range (0,21):
        file.write(p1+str(i)+'\n')
    file.close()

# Step2: add password files
def pwds():
    for i in range(0,21):
        path = './node'+str(i)+'/password.txt'
        file = open(path,'w')
        file.write('node'+str(i))
        file.close()

# Step3: write primary toml
def toml():
    for i in range(0,21):
        path = './node' + str(i) + '.toml'
        file = open(path,'w')
        s1 = '[parity] \nchain = "21chain.json" \nbase_path = "node'+str(i)+'"\n\n'
        s2 = '[network] \nport = ' + str(30300+i) + '\nid = ' + str(2025) + '\nreserved_only = false\n\n'
        s3 = '[rpc]\nport = '+str(8650+i)+'\napis =["all"]\n\n'
        s4 = '[websockets]\ndisable = false \nport = ' + str(8750+i)+'\n\n'
        file.write(s1+s2+s3+s4)
        file.close()

# Step4: create accounts
def acc():
    file = open('./accounts.sh','w')
    for i in range(0,21):
        p = 'curl --data \'{"jsonrpc":"2.0","method":"parity_newAccountFromPhrase","params":["node'+str(i)+'", "node'+str(i)+'"],"id":0}\' -H "Content-Type: application/json" -X POST localhost:' + str(
            8650+ i) +'\n'
        file.write(p)
    file.close()

def create_accs():
    file = open('./create_accs.sh','w')
    file.write('#!/bin/bash\n\n')
    for i in range(0,21):
        p = 'gnome-terminal -t "node'+str(i)+'" -x bash -c "openethereum --config node'+str(i)+'.toml;exec bash;"\n'
        file.write(p)
    file.close()

def rmdb():
    file = open('./removedb.sh','w')
    for i in range(0,21):
        file.write('rm -r ./node'+str(i)+'/chains/chain21/db/b67df7bf761dac1a\n')
    file.close()

def qryacc():
    file = open('./qryacc.sh','w')
    for i in range(0,21):
        file.write('curl --data \'{"method":"eth_accounts","params":[],"id":1,"jsonrpc":"2.0"}\' -H "Content-Type: application/json" -X POST localhost:'+str(8650+i)+'\n')
    file.close()

sealers = ["0x00bd138abd70e2f00903268f3db08f2d25677c9e",
            "0x00aa39d30f0d20ff03a22ccfc30b7efbfca597c2",
            "0x002e28950558fbede1a9675cb113f0bd20912019",
            "0x00a94ac799442fb13de8302026fd03068ba6a428",
            "0x00d4f0e12020c15487b2a525abcb27de647c12de",
            "0x001f477a48a01d2561e324f874782b2dd8167772",
            "0x006137d98307ab6691ccedb7a10b295da8ae1035",
            "0x003f3b1f635b2dd9a4518c33098e5f72214d6a1e",
            "0x008272a8cfd2d3d0f3edc823b1bb729cb73f09db",
            "0x001ce0f63558e2fe10806d132d64d2b2f63ef64e",
            "0x0038658156bcb555c1aa24d1adabb57c36fbcd6d",
            "0x006a8e26c9653d22f1cadb22a81428deaa8554be",
            "0x00c3ca2fd819f4d2ea30c9fd99bf80c7c86f1f25",
            "0x00734b960d1edd54e50192e47acfdc8af0fbbd20",
            "0x002db24c08ed9397bc77a554e55f80d56be7b15f",
            "0x004f49d9267bce6bdefc0fe9065269fa5d24ead9",
            "0x00da2f656d0ae044234479e93d2006798046d6cd",
            "0x004edc8b40e4c8210e7c25cd9236f2461bbf1ada",
            "0x00a6a2655ad6707e925bb6949a933f05690288bb",
            "0x002d7b6716b90ef6a10c9ecbf4bf1056cd62a41c",
            "0x0049555fbcd81a300481f8bab352f2bd0679140e"]

# Step5 update toml
def update_toml():
    for i in range(0,21):
        path = './node' + str(i) + '.toml'
        file = open(path,'w')
        s1 = '[parity] \nchain = "21chain.json" \nbase_path = "node'+str(i)+'"\n\n'
        s2 = '[network] \nport = ' + str(30300+i) + '\nid = ' + str(2025) + '\nreserved_only = false\nreserved_peers = "myPrivateNetwork.txt"\n\n'
        s3 = '[rpc]\nport = '+str(8650+i)+'\napis =["all"]\n\n'
        s4 = '[websockets]\ndisable = false \nport = ' + str(8750+i)+'\n\n'
        s5 = '[account]\npassword = ["node'+str(i)+'/password.txt"]\nunlock = ["'+sealers[i]+'"]\n\n'
        s6 = '[mining]\nreseal_on_txs = "none"\nforce_sealing = true\nauthor = "'+sealers[i]+'"\nengine_signer = "'+sealers[i]+'"\n\n'
        file.write(s1+s2+s3+s4+s5+s6)
        file.close()

def nohuprun():
    file = open('./nohuprun.sh','w')
    p1 = '/data/xr/openethereum-3.3.4_82s/target/release/openethereum --config node0.toml  1>logs/node0.log 2>&1 & echo $! > nodes.pid\n'
    file.write(p1)
    for i in range(1,21):
        p2 = 'nohup   /data/xr/openethereum/target/release/openethereum --config node'+str(i)+'.toml  1>logs/node'+str(i)+'.log 2>&1 & echo $! > nodes.pid\n'
        file.write(p2)
    file.close()

update_toml()
