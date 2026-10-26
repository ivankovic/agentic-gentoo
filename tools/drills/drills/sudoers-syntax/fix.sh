#!/bin/bash
printf 'Defaults env_keep += "HTTP_PROXY"\n' > /etc/sudoers.d/proxy && visudo -c
