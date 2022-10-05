CC := musl-gcc
CXX := /bin/false
CFLAGS := -std=c11 -O2 -Wall -Wextra -Werror -fPIE -pie -Iinclude/

ifneq ($(DEBUG), "")
CFLAGS += -DNDEBUG
endif

ifneq ($(findstring $(MAKEFLAGS),s),s)
ifndef V
	QUIET_CC = @echo '    '   CC $@;
endif
endif

HEADERS_URL ?= https://nl.alpinelinux.org/alpine/edge/main/x86_64/linux-headers-5.19.5-r0.apk
HEADERS_SHA256 ?= b2cfa4ca25d9d145c25488481736b5a6709049c70405b4b40a12609d0a92e655

MAIN := pump
SRC_DIR ?= src
LIBURING_DIR ?= $(CURDIR)/external/liburing
UNPACKED_HEADERS := $(CURDIR)/external/linux

OBJECTS = $(addprefix $(SRC_DIR)/,$(MAIN).o forward.o network.o)

.DEFAULT_GOAL = all
.PHONY: all
all: $(MAIN)

$(SRC_DIR)/$(MAIN).o: $(SRC_DIR)/$(MAIN).c
	$(QUIET_CC)$(CC) $(CFLAGS) \
	    -o $@ -c $^

$(SRC_DIR)/forward.o: $(SRC_DIR)/forward.c
	$(QUIET_CC)$(CC) -O2 -Wall -Wextra -Werror -fPIE -pie \
		-I"$(UNPACKED_HEADERS)/usr/include" \
        -I"$(LIBURING_DIR)/src/include/" \
        -Iinclude/ \
        -lstdthreads \
	    -o $@ -c $^

$(SRC_DIR)/network.o: $(SRC_DIR)/network.c
	$(QUIET_CC)$(CC) $(CFLAGS) \
		-I"$(UNPACKED_HEADERS)/usr/include" \
	    -o $@ -c $^

%.o: %.c
	$(QUIET_CC)$(CC) $(CFLAGS) -o $@ -c $^

$(MAIN): $(UNPACKED_HEADERS) liburing $(OBJECTS) $(OBJECTS_EXT)
	$(QUIET_CC)$(CC) $(CFLAGS) \
		-I"$(UNPACKED_HEADERS)/usr/include" \
		-I"$(LIBURING_DIR)/src/include/" \
		-static -o $@ $(wordlist 3, $(words $^), $^) \
		"$(LIBURING_DIR)/src/liburing.a"
	@# default musl libs on some distros have debug symbols, lets strip them (and everything else)
	strip $@

$(UNPACKED_HEADERS):
	wget -q -O "headers" $(HEADERS_URL)
	echo $(HEADERS_SHA256) "headers" | sha256sum -c || (echo "Headers apk checksum verification failed!" && exit 1)
	$(RM) -rf $(UNPACKED_HEADERS)
	mkdir $(UNPACKED_HEADERS)
	tar --warning=no-unknown-keyword -C $(UNPACKED_HEADERS) -vxzf headers >/dev/null
	$(RM) headers

liburing: $(UNPACKED_HEADERS)
	@echo build liburing
	git clone --depth 1 --branch liburing-2.1 https://github.com/axboe/liburing.git $(LIBURING_DIR) || true
	(cd $(LIBURING_DIR) && CC=$(CC) CXX=$(CXX) ./configure > /dev/null)
	$(MAKE) -e CC=$(CC) -e CFLAGS="-I'$(UNPACKED_HEADERS)/usr/include' -O2 -fPIE -pie" -C "$(LIBURING_DIR)/src" all

.PHONY: clean
clean:
	$(RM) $(MAIN) $(SRC_DIR)/*.o $(TEST_DIR)/*.o *.o
	$(MAKE) -s -C external/liburing/src clean
	$(MAKE) -s -C $(LIBURING_DIR) clean

.PHONY: distclean
distclean:
	$(RM) -rf $(UNPACKED_HEADERS)
	$(RM) -rf $(LIBURING_DIR)
	git submodule foreach git reset --hard
