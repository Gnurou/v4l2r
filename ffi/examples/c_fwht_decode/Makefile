# This requires a debug build to compile, so make sure to run "cargo build" from
# the top of the repository beforehand (or "cargo build --release" if you want
# to build a release version).
#
# Also requires the vicodec kernel module to be loaded with multiplanar support
# (i.e. "modprobe vicodec multiplanar=1").
all: fwht_decode

fwht_decode: fwht_decode.c
	cc -Wall $< -o$@ ../../../target/debug/libv4l2r_ffi.a -I../../ -lpthread -ldl -lrt

fwht_decode_release: fwht_decode.c
	cc -Wall -O3 $< -o$@ ../../../target/release/libv4l2r_ffi.a -I../../ -lpthread -ldl -lrt

clean:
	rm -f fwht_decode fwht_decode_release
