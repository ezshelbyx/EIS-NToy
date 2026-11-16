# EIS NToy

Ventoy-style multiboot USB creation tool.

## Overview

EIS NToy is a multiboot USB creation tool inspired by Ventoy. It allows users to copy multiple ISO files to a USB drive and boot from any of them without reformatting.

## Stack

- Rust core for USB partitioning and boot sector management
- Native Linux GUI (GTK4 or egui)
- Safety-first: verifies target device is USB before any write

## Safety

- Verifies target is a removable USB device before any write
- Requires explicit user confirmation for destructive operations
- Never targets internal storage
- Preserves existing files on the USB drive

## Roadmap

- [ ] M01: Project scaffold
- [ ] M02: USB device detection and verification
- [ ] M03: Partition scheme (Ventoy-style dual-partition)
- [ ] M04: Boot sector and GRUB installation
- [ ] M05: ISO file management (copy, list, remove)
- [ ] M06: Boot menu generation
- [ ] M07: UEFI and Legacy BIOS support
- [ ] M08: Verification and integrity checking
