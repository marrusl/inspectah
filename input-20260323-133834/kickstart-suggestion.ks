# Kickstart suggestion — review and adapt for your environment
# These settings belong at deploy time, not baked into the image.

# network --hostname=input

# --- Human users (deploy-time provisioning) ---
user --name=mrussell --uid=1000 --gid=1000 --shell=/bin/bash --homedir=/home/mrussell
user --name=appuser --uid=1001 --gid=1001 --shell=/bin/bash --homedir=/home/appuser
# Set passwords interactively or via --password/--iscrypted

# --- Examples ---
# network --bootproto=dhcp --device=eth0
# network --hostname=myhost.example.com
# network --bootproto=static --ip=192.168.1.10 --netmask=255.255.255.0 --gateway=192.168.1.1
