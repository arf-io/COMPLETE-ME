while IFS= read -r line
  echo '['"$line"']'
end <input.txt
