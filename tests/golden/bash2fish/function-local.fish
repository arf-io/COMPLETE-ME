function f
  set -l x 'inner'
  echo $x
end
set x 'outer'
f
echo $x
