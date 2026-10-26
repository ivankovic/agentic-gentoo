#!/bin/bash
case $KIND in
	locked) usermod -U "$U_NAME" ;;
	expired) usermod -e '' "$U_NAME" ;;
esac
