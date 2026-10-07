#!/usr/bin/env bash
exec timeout -v "$2" bash -c "$1" 2>&1
