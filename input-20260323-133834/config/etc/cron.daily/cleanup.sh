#!/bin/sh
# Cleanup script — driftify synthetic fixture
find /tmp -name 'myapp-*' -mtime +7 -delete
