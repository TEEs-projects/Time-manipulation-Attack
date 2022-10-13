usrs=["0x005b0fbe9a9a53e66aca408e9dc2f9c53cbd6665",
    "0x00e46a5a194748871d4d17ac88d657f63b1c50e3",
    "0x00379d1ae3b1def5241a44369397a4dadb1dff64",
    "0x0054076b6784fc25baf961db2ebc760a49a14379",
    "0x0032d84dff7be846333990d48d05db2a670089ad",
    "0x0063ec1c2b77e2d1f9cd937e2b158a988e3f77c0",
    "0x00d49a6f587bfa28535af292e335af82692d78d8",
    "0x00f0d1c4e8c7ac0f60768dd768411cce48bd4100",
    "0x002bbd28290355640f455e7535313d17a8c4f98f",
    "0x00f2d36b1e343fdcdfcb733710944bfc1cd04413",
    "0x002b7415ba7c4373da1117e3d40809ec6f17e646",
    "0x0010271e70d0b66490c12f0a1ad64db6e17adb83"
    #"0x00ac9126e880cd1badf99be4b347876776088d1b",
    #"0x0070ac59613e2f544bf7dad12838b0227e419b96",
    #"0x00fa0e5c5cb9163378df676c7f3ae201d69105c7"
    ]

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

for i in range(0,5):
    dir = '/data/xr/testchain/sends/send'+str(i)+'.sh'
    file=open(dir,'w+')
    p0 ='for ((i=1;i<1000;i++)); do  echo $i; \n'
    p1='time curl --data \'{"method":"eth_sendTransaction","params":[{"from":"'
    p2='","to":"'
    p3='","gas":"0x21000","gasPrice":"0x20","value":"0x22"}],"id":1,"jsonrpc":"2.0"}\' -H "Content-Type: application/json" -X POST localhost:'
    p4='; \n'
    p5='done'
    port=8671+i%5
    file.write("#!/bin/bash\n")
    file.write(p0+p1+usrs[i%5]+p2+usrs[(i%5+2)%5]+p3+str(port)+p4)
    #file.write(p1+usrs[i%5]+p2+usrs[(i%5+1)%5]+p3+str(port)+p4)
    file.write(p5)
    file.close()

file=open('/data/xr/testchain/send.sh','w+')
for i in range(0,5):
    p='chmod +x   ./sends/send'+str(i)+'.sh\n'
    file.write(p)
file.write("echo 'nohup start'+$(date +%H:%M:%S)>trans_out.txt\n")
for i in range(0,5):
    p='  ./sends/send'+str(i)+'.sh  &\n'
    file.write(p)
file.write("wait\necho 'nohup end'+$(date +%H:%M:%S)>>trans_out.txt\n")
file.close()

