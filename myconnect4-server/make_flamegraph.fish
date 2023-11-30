#!/bin/fish

eval "perf record --call-graph dwarf $argv"
perf script | inferno-collapse-perf > stacks.folded
cat stacks.folded | inferno-flamegraph > flamegraph.svg
