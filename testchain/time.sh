start='date +%Y-%m-%d %H:%M:%S'
end='date +%Y-%m-%d %H:%M:%S'
startsec=$(date --date="$start" +%s);
endsec=$(date --date="$end" +%s);
echo 'period= "$((endsec-startsec))"s'
