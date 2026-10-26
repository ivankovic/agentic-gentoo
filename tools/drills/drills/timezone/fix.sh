#!/bin/bash
declare -A tz=([Zagreb]=Europe/Zagreb ["New York"]=America/New_York [Tokyo]=Asia/Tokyo [Berlin]=Europe/Berlin ["Sao Paulo"]=America/Sao_Paulo)
timedatectl set-timezone "${tz[$CITY]}"
