# coding=utf-8
import time

file_in = open('/data/xr/testchain/qry_result.txt','r')
file_out = open('/data/xr/testchain/result_readable.txt','w')
file_out1 = open('/data/xr/testchain/result_indexes.txt','w')
indexes =[]
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

file_content = file_in.read()
result_list = file_content.split('\n')
# 向文件输出index矩阵
def print_index():
    for i in range(0,len(indexes)):
        file_out1.write(str(indexes[i])+'\t')
        if (i+1)%21==0 :
            file_out1.write('\n')

def write_result(result_n):
    resultline = result_n
    # blocknumber
    blocknumber_hex = resultline[resultline.find("number") + 9:resultline.find("parentHash") - 3]
    blocknumber = int(blocknumber_hex,16)
    # block hash
    blockhash = resultline[resultline.find("hash") + 7:resultline.find("logsBloom") - 3]
    # author
    author = resultline[resultline.find("author") + 9:resultline.find("author") + 51]
    index = sealers.index(author)
    indexes.append(index)
    # step
    step = resultline[resultline.find("step") + 7:resultline.find("timestamp") - 3]
    # daytime
    timestamp_hex = resultline[resultline.find("timestamp") + 12:resultline.find("totalDifficulty") - 3]
    timestamp_oct = int(timestamp_hex,16)
    local = time.localtime(timestamp_oct)
    daytime = time.strftime("%Y-%m-%d %H:%M:%S",local)
    p = daytime+'\t '+str(index)+'\t'+step+'\t'+'#'+str(blocknumber)+'\t'+'#'+blocknumber_hex+'\t'+blockhash[:6]+'...'+blockhash[62:]+'\n'
    file_out.write(p)

def index_to_string(indexes):
    indexstr = ''
    for i in range(0,len(indexes)):
        indexstr = indexstr + str(indexes[i])+' '
    return indexstr

def cut_indexstr_234(indexstr):
    indexstr.strip('2 3 4')
    return indexstr

def count_loss_of_0(str):
    return str.count('20 1')+str.count('20 2')+str.count('20 3')
def count_loss_of_1(str):
    return str.count('0 2')+str.count('0 3')+str.count('0 4')
def count_loss_of_2(str):
    return str.count('1 3')+str.count('0 3')+str.count('0 4')
def count_loss_of_3(str):
    return str.count('2 4')+str.count('0 4')++str.count('1 4')
def rough_cycles(str):
    return str.count('15 16')


# call function to write results
for i in range (0,len(result_list)-1):
    write_result(result_list[i])
# write pure indexes matrix
print_index()
# transfer index[] to string
indexstr = index_to_string(indexes)
# 去掉连续的234
#indexstr1 = cut_indexstr_234(indexstr)
# 输出到矩阵文件
#file_out1.write(indexstr1+'\n')
loss = 'loss of 0 (malicious) = '+str(count_loss_of_0(indexstr))+'\n'+'loss of 1 = '+str(count_loss_of_1(indexstr))+'\n'\
    +'loss of 2 = '+str(count_loss_of_2(indexstr))+'\n'\
    +'loss of 3 = '+str(count_loss_of_3(indexstr))+'\n'\
    +'approximately '+str(rough_cycles(indexstr))+' rounds\n'
total = '0 (malicious): '+str(indexstr.count(' 0 '))+'\n'+'1: '+str(indexstr.count(' 1 '))+'\n'+'2: '+str(indexstr.count(' 2 '))+'\n'+'3: '+str(indexstr.count(' 3 '))+'\n'+'4: '+str(indexstr.count(' 4 '))+'\n'
file_out1.write(loss+total)


file_out1.close()
file_in.close()
file_out.close()