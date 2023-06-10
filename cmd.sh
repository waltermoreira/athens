#!/bin/sh

echo "Hello World to stdout" >&1
sleep 1
echo "Hello World to stderr foo bar baz spam eggs dsjk jslfjsl jsdlfj sdjfs jksd" >&2
sleep 1
echo "More text"
ls -l
sleep 1
echo "Even more (to stderr)" >&2
echo "finish"
exit 2