Thank you for all your comments. We endeavor to address all the comments below. 

## Scope Clarification (Review 1):

In terms of scope, we do feel that our paper is closely related to the security and privacy trust track of WWW, as blockchain is a distributed system that is built on web-based technologies such as TCP/IP and peer-to-peer networks and is also closely related to web tech such as Web3. The ‘relevant topics’ of WWW also clearly specify ‘blockchains and distributed ledgers’ and ‘cryptocurrencies and smart contracts’. 

In fact, several blockchain papers have been published with WWW in recent years. Below we list a few of them. 

1. Lei Wu, et al. Towards Understanding and Demystifying Bitcoin Mixing Services. WWW 21.  
2. Christian Aebeloe, et al. ColChain: Collaborative Linked Data Networks. WWW 21. 
3. Yuan, Liang, et al. Coopedge: A decentralized blockchain-based platform for cooperative edge computing. WWW 21.
4. Manoharan Ramachandran, et al. Towards Complete Decentralised Verification of Data with Privacy: Different ways to connect Solid Pods and Blockchain. WWW 20. 

We also find that WWW (esp. the security, privacy & trust track) favours the papers that discover risks and attacks & defence of known technologies (e.g., data analytics and ML). For instance, 

1. Fang, Minghong, et al. Data poisoning attacks and defences to crowdsourcing systems. WWW  21.
2. Xu, Chang, et al. A Targeted Attack on Black-Box Neural Machine Translation with Parallel Data Poisoning. WWW 21.

Additionally, in our attacks, the mining rights of honest nodes are deprived, and malicious mining nodes may reorder, drop, or censor users’ transactions, causing the browser-based wallets and page-based DApps to fail to receive correct receipts (or get reduced profit). 

Therefore, we do feel that our paper is a good fit to WWW, given that it studies a type of attack to PoA Aura via manipulation of the timestamps, and the attacks themselves and their impacts are both closely related to web-based technologies. 


## Disclosure Clarification (Review 1 & 3):

Unlike conventional commercial software that runs on closed source (or strictly licensed) code, blockchain projects are typically open source, and the developers work in an open community (with CC0 license). It is common in the blockchain-related field to present attacks to existing algorithms/protocols. Attacks are useful for one to discover the potential risks of a system even if the security assumptions are not violated. It is not ‘required’ for papers like this to be published before receiving feedbacks for the disclosure. See some examples below. 

1. Eyal, Ittay, et al. Majority is not enough: Bitcoin mining is vulnerable. Communications of the ACM 2018
2. Heilman, Ethan, et al. Eclipse attacks on Bitcoin’s peer-to-peer network. USENIX Security 15
3. Liyi Zhou, et al. High-frequency trading on decentralized on-chain exchanges. IEEE S&P 2021

An example: Sandwich attack in DeFi protocols (see [3] above by Liyi Zhou et al.)  is an attack that poses threats more than billions of USD (as they claimed in paper). However, they merely mentioned the disclosure as “We disclosed our preliminary results to Uniswap on 18th of November 2019, which allowed tightening the trader protections” (P429, Footnote 4). The paper is published before the authors received responses from the team that maintained its targeted protocol, Uniswap. 

In fact, most attacks in the blockchain field cannot provide actual disclosure from the official teams as a lot of the projects are managed by many volunteers and any patches need to go through a long period of review process before they are accepted. Furthermore, although attacks cause potential risks to the system, it does not mean it is easy to launch such attacks. We believe that presenting such attacks are helpful to point out the threats of systems in use. 


As claimed in this paper, we have tried to reach out to four mainstream Aura-based projects via email, including Nethermind, Substrate, and OpenEthereum Founder (Gavin Wood), all the channels we are aware of. We will keep the disclosure statement (either on our personal websites or on eprint) updated once we hear back from the team. 


## Attack Impact Clarification (Review 3):
The data of affected market cap are calculated in the most pessimistic way by gathering all the mainstream projects that adopt Aura or fork OpenEthereum. To make it more rigorous, we will adopt the term “the potentially affected market” rather than “the affected market” throughout all the paper.

## Related Work (Review 3):
We did not expand the related work section partly because our attacks utilize a strategy that  has low degrees of correlation with previous attacks. We have compared our attacks with some mainstream PoA attacks in Table1 and pointed out how our attacks different from them. We are happy to expand it in the revised version or post a full paper on eprint.  

## Other comments:

**Review 2#1**: Even if Aura is proposed outside academia, it has been widely adopted and has been well studied (e.g., clone attack in NDSS 20). We believe our work can push forward this field. 

**Review 2#2**: The drifting tolerance is only applied in time-based PoA leader election schemes, namely, PoA Aura. Although this does not affect all implementations, some major projects such as OpenEthereum and its variant are affected. We will clearly point out the scope of the flaw in the revised version.

**Review 2#3**: We will make sure to fix all the typos and clarify strategies when presenting the quantitative results.

**Review 3#2**: We are happy to expand the section and provide detailed countermeasures in the revised version.
