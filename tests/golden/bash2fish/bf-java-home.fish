if [ -z (set -q JAVA_HOME && echo "$JAVA_HOME" || echo '') ]
  set -gx JAVA_HOME '/bla/lib/openjdk'
end
