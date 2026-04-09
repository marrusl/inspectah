#!/bin/sh
export APP_ENV=production
export LOG_LEVEL=info

# BEGIN DRIFTIFY fake-secrets-env
export REDIS_URL=REDACTED_REDIS_PASSWORD_ec7a839aredis.internal:6379
# END DRIFTIFY fake-secrets-env
