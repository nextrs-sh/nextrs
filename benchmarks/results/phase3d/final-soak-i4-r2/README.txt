commit b09431d (b09431d0b9377e3ca8f20085456ec3667b9f9a9b)
binary /home/dennispaler/Documents/OpenSource/nextrs/target/phase3d-soak/nextrs-621da2042201
binary_sha256 621da20422015abc92114887c8d794523e7c5c61d5ec1e954625af1c29e34bfc
version nextrs 0.1.0-rc.1
isolates 4; minutes 360; idle 10
started 2026-11-19T04:02:08Z; load 1.27 10.15 21.78
node v24.18.0; host 7.3.4-300.fc45.x86_64; cpu Intel(R) Core(TM) i5-9400F CPU @ 2.90GHz
interrupted 2026-11-19T14:21:59Z: the soak processes ended with the Claude session at about 04:38Z (34 min in). Before that, from 04:25Z, an unrelated compile and swap-in drove load to 77-93 with I/O pressure full 21-31 %; 8 pages-revalidate requests hit the 15 s deadline (504) in that window. Superseded by r3.
