#!/bin/sh
export HTTP_PROXY=http://proxy.internal:3128
export HTTPS_PROXY=http://proxy.internal:3128
export NO_PROXY=localhost,127.0.0.1,.internal,github.com,githubusercontent.com,ghcr.io,quay.io,registry.redhat.io
