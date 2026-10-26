#!/bin/bash
set -e
declare -A tz=([Zagreb]=Europe/Zagreb ["New York"]=America/New_York [Tokyo]=Asia/Tokyo [Berlin]=Europe/Berlin ["Sao Paulo"]=America/Sao_Paulo)
[[ $(readlink -f /etc/localtime) == "/usr/share/zoneinfo/${tz[$CITY]}" ]]
[[ $(date +%z) == "$(TZ=${tz[$CITY]} date +%z)" ]]
