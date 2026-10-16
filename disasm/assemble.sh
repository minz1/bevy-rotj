#!/bin/sh
# Generates the Batman: Return of the Joker (USA) disassembly from your ROM
# (gen/generate.py), assembles it into rotj.nes and verifies it is byte-identical
# to that ROM. Acceptance test. The ROM is $ROTJ_ROM, else ../raw/.
# Requires ca65 / ld65 (cc65); default to ~/.local/bin, override with $CA65/$LD65.
set -eu

cd "$(dirname "$0")"
CA65="${CA65:-$HOME/.local/bin/ca65}"
LD65="${LD65:-$HOME/.local/bin/ld65}"
ROM="${ROTJ_ROM:-../raw/Batman - Return of the Joker (USA).nes}"
OUT="rotj.nes"
mkdir -p obj
python3 gen/generate.py "$ROM"

for f in header.s chr.s bank00.s bank01.s bank02.s bank03.s bank04.s bank05.s \
         bank06.s bank07.s bank08.s bank09.s bank0A.s bank0B.s bank0C.s \
         bank0D.s bank0E.s bank0F.s; do
    "$CA65" -o "obj/${f%.s}.o" "$f"
done
# shellcheck disable=SC2046
"$LD65" --config rotj.cfg -o "$OUT" obj/header.o obj/bank00.o obj/bank01.o \
    obj/bank02.o obj/bank03.o obj/bank04.o obj/bank05.o obj/bank06.o \
    obj/bank07.o obj/bank08.o obj/bank09.o obj/bank0A.o obj/bank0B.o \
    obj/bank0C.o obj/bank0D.o obj/bank0E.o obj/bank0F.o obj/chr.o

if cmp -s "$OUT" "$ROM"; then
    echo "OK byte-identical ($(sha1sum < "$OUT" | cut -d' ' -f1))"
else
    echo "FAIL: $OUT is not byte-identical to the source ROM" >&2
    cmp "$OUT" "$ROM" || true
    exit 1
fi
