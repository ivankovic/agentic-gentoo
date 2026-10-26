#!/bin/bash
systemctl unmask "$SVC_NAME.service" && systemctl enable --now "$SVC_NAME.service"
