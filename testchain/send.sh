chmod +x   ./sends/send0.sh
chmod +x   ./sends/send1.sh
chmod +x   ./sends/send2.sh
chmod +x   ./sends/send3.sh
chmod +x   ./sends/send4.sh
echo 'nohup start'+$(date +%H:%M:%S)>trans_out.txt
  ./sends/send0.sh  &
  ./sends/send1.sh  &
  ./sends/send2.sh  &
  ./sends/send3.sh  &
  ./sends/send4.sh  &
wait
echo 'nohup end'+$(date +%H:%M:%S)>>trans_out.txt
