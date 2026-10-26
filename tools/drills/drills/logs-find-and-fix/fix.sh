#!/bin/bash
sed -i 's/^threads = many$/threads = 4/' /etc/worker.conf && systemctl restart worker.service
