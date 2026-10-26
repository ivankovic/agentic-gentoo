#!/bin/bash
systemctl unmask hello.service && systemctl enable --now hello.service
