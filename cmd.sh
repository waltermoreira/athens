#!/bin/sh

echo "Hello World to stdout" >&1
sleep 2
echo "Hello World to stderr foo bar baz spam eggs dsjk jslfjsl jsdlfj sdjfs jksd" >&2
sleep 1
echo "More text"
sleep 1
echo "Even more (to stderr)" >&2
sleep 1
echo "Almost there"
sleep 1
echo "finish"
sleep 2
exit 1