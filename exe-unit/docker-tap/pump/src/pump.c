#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <unistd.h>

#include "forward.h"
#include "network.h"

#define MIN_MTU (576 + 14)

static int g_net_read_fd = -1;
static int g_net_write_fd = -1;
static int g_net_tap_fd = -1;

//extern int mknod(const char *, mode_t, dev_t);

static void die(void) {
    if (g_net_tap_fd > 0) close(g_net_tap_fd);
    if (g_net_read_fd > 0) close(g_net_read_fd);
    if (g_net_write_fd > 0) close(g_net_write_fd);

    exit(1);
}

#define CHECK(x) ({                                                     \
    __typeof__(x) _x = (x);                                             \
    if (_x == -1) {                                                     \
        fprintf(stderr, "Error at %s:%d: %m\n", __FILE__, __LINE__);    \
        die();                                                          \
    }                                                                   \
    _x;                                                                 \
})

//int create_chardev(char *path) {
//    int ret = 0;
//    if ((ret = mknod(path, S_IFCHR | S_IRWXU | S_IRWXG | S_IRWXO, makedev(1, 3))) > 0) {
//        return ret;
//    }
//    if ((ret = chmod(path, 0777)) < 0) {
//        return ret;
//    }
//    return ret;
//}

int main(int argc, char* argv[]) {
    if (argc < 7) {
        fprintf(stderr, "Usage:\n");
        fprintf(stderr, "%s [tap_name] [read.sock] [write.sock] [ip] [gw] [mtu]", argv[0]);
        fflush(stderr);

        die();
    }

    char *tap = argv[1];
    char *read_sock = argv[2];
    char *write_sock = argv[3];
    char *ip = argv[4];
    char *gw = argv[5];
    int mtu = atoi(argv[6]);

    if (mtu < MIN_MTU) {
        fprintf(stderr, "Invalid mtu: %d (< %d):\n", mtu, MIN_MTU);
        fflush(stderr);

        die();
    }

    fprintf(stdout, "starting %s...\n", argv[0]);
    fprintf(stdout, "\ttap: \t\t%s\n", tap);
    fprintf(stdout, "\tsocket r: \t%s\n", read_sock);
    fprintf(stdout, "\tsocket w: \t%s\n", write_sock);
    fprintf(stdout, "\tip: \t\t%s\n", ip);
    fprintf(stdout, "\tgw: \t\t%s\n", gw);
    fprintf(stdout, "\tmtu: \t\t%d\n", mtu);

    fprintf(stdout, "-> socket\n");
    fflush(stdout);

    CHECK(g_net_read_fd = net_create_unix_socket(read_sock, 0));
    CHECK(g_net_write_fd = net_connect_unix_socket(write_sock));

    fprintf(stdout, "-> tap\n");
    fflush(stdout);

    CHECK(g_net_tap_fd = net_create_tap(tap));

    fprintf(stdout, "-> pump\n");
    fflush(stdout);

    pump(
        mtu,
        g_net_tap_fd,
        g_net_read_fd,
        g_net_write_fd,
        read_sock,
        write_sock
    );

    fprintf(stdout, "... done.\n");
    fflush(stdout);

    return 0;
}
