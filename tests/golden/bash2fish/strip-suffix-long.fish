set p 'a.b.c'
echo (string replace -r '(\\..*)$' '' "$p")
