function show
  for x in "$argv"
    echo '['"$x"']'
  end
end
show 'a b' c
