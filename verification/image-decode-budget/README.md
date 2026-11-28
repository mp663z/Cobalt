Fuzzfixture SHA25602ebba946ffd33d43dcc65fc51508321fdb742516c2d28b3a92463a0929a0fe8. 13290B, claimed320x240 progressiveJPEG. Plainx86release firstdecode noresultfor>81s aftercompile, killedat100souterbudget. InvalidreservedFF93 marker atoffset1950 beforetruncatedsegment. Addedlinear markerframingpreflight and64scan ceiling beforeprobe/grey/colourdecoder. Fixedreleasegrey4us/colour1us refusal. Ordinarygeneratedprogressive32x24 stilldecodes. Unit25/integration3pass. ARMreleaseprobebuildpass; qemu notonPATH, hardwareunverified. Not a generalCPUdeadline or completeJPEGvalidityproof. No dependency/unsafe addition.

Compatibility follow-up: browser's existing truncated.jpg intentionally recovers
an entropy EOF. Preserve this case only; incomplete segments/invalid reserved
markers remain refused. Added explicit regression, image25unit+4integration pass,
browserpicturematrix14pass. Originalpicturetests unchanged. Not broadJPEG validity
or universaldecode-time bound.
