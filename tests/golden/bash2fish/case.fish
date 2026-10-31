set x 'apple'
switch "$x"
case 'a*'
  echo starts-a
case 'b*' 'c*'
  echo b-or-c
case '*'
  echo other
end
