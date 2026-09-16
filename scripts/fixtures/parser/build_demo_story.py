#!/usr/bin/env python3
"""Build the parser demo story, byte for byte the zvm test fixture's shape.

A minimal synthetic v5 Z-machine story: read one word, print "accepted",
quit. It exists so simulator routes can open a real story without a device
push. The checksum header word is computed before the program bytes land,
matching apps/parser/src/zvm (the story never runs the verify opcode).
"""
from pathlib import Path
import sys

OUT = Path(__file__).with_name('story-demo.z5')


def encode_dictionary_word(text, version):
    wanted = 6 if version <= 3 else 9
    zchars = []
    for byte in text[:wanted]:
        if 0x61 <= byte <= 0x7A:
            zchars.append(byte - 0x61 + 6)
        elif 0x41 <= byte <= 0x5A:
            zchars += [4, byte - 0x41 + 6]
        elif byte == 0x20:
            zchars.append(0)
        else:
            zchars += [5, 6, byte >> 5, byte & 0x1F]
    zchars += [5] * (wanted - len(zchars))
    encoded = bytearray()
    for index in range(0, wanted, 3):
        value = (zchars[index] << 10) | (zchars[index + 1] << 5) | zchars[index + 2]
        if index + 3 == wanted:
            value |= 0x8000
        encoded += value.to_bytes(2, 'big')
    return encoded


def story(version):
    bytes_ = bytearray(0x300)
    bytes_[0] = version
    bytes_[2:4] = (1).to_bytes(2, 'big')
    bytes_[6:8] = (0x40).to_bytes(2, 'big')
    bytes_[8:10] = (0x180).to_bytes(2, 'big')
    bytes_[0x0A:0x0C] = (0x100).to_bytes(2, 'big')
    bytes_[0x0C:0x0E] = (0x140).to_bytes(2, 'big')
    bytes_[0x0E:0x10] = (0x200).to_bytes(2, 'big')
    bytes_[0x12:0x18] = b'260901'
    scale = {3: 2, 5: 4, 8: 8}[version]
    bytes_[0x1A:0x1C] = (len(bytes_) // scale).to_bytes(2, 'big')
    checksum = sum(bytes_[0x40:]) & 0xFFFF
    bytes_[0x1C:0x1E] = checksum.to_bytes(2, 'big')
    program = bytearray()
    program.append(0xB2)
    program += encode_dictionary_word(b'hello', version)
    program.append(0xBB)
    program.append(0xBA)
    bytes_[0x40:0x40 + len(program)] = program
    return bytes_


def input_story():
    bytes_ = story(5)
    bytes_[0x40] = 0xE4
    bytes_[0x41] = 0x5F
    bytes_[0x42] = 0x80
    bytes_[0x43] = 0xA0
    bytes_[0x44] = 0x10
    bytes_[0x45] = 0xB2
    encoded = encode_dictionary_word(b'accepted', 5)
    bytes_[0x46:0x46 + len(encoded)] = encoded
    bytes_[0x46 + len(encoded)] = 0xBA
    bytes_[0x80] = 32
    bytes_[0xA0] = 4
    bytes_[0x180] = 0
    bytes_[0x181] = 6
    bytes_[0x182:0x184] = (0).to_bytes(2, 'big')
    return bytes_


def main():
    if OUT.exists() and '-f' not in sys.argv:
        sys.exit(f'refusing to overwrite {OUT}; pass -f to regenerate')
    OUT.write_bytes(input_story())
    print(f'wrote {OUT} ({OUT.stat().st_size} bytes)')


if __name__ == '__main__':
    main()
