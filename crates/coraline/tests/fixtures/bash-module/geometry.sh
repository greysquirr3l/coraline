#!/usr/bin/env bash
source ./helpers.sh

RADIUS=2.0

area() {
    echo $((RADIUS * RADIUS))
}

total() {
    area
    echo "total"
}
