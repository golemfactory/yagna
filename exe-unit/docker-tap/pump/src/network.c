#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/if.h>
#include <linux/if_arp.h>
#include <linux/if_tun.h>
#include <linux/ipv6_route.h>
#include <linux/route.h>
#include <linux/socket.h>
#include <linux/string.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/ioctl.h>
#include <sys/select.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

#include "network.h"

static unsigned int alias_counter = 0;


struct ifreq6_stub {
    struct in6_addr addr;
    uint32_t prefixlen;
    int32_t ifindex;
};

int parse_prefix_len(const char *ip) {
    char *cp;
    if ((cp = strchr(ip, '/'))) {
        return atol(cp + 1);
    }
    return -1;
}

int net_if_alias(struct ifreq *ifr, const char *name) {
    const int suffix_len = 5;
    if (strlen(name) >= sizeof(ifr->ifr_name) - suffix_len) {
        return -1;
    }
    snprintf(ifr->ifr_name, sizeof(ifr->ifr_name) - 1,
            "%s:%d", name, ++alias_counter);
    return 0;
}

int net_create_lo(char *name) {
    struct ifreq ifr;
    int fd, ret;

    if ((fd = socket(PF_INET, SOCK_DGRAM, IPPROTO_IP)) < 0) {
        return fd;
    }

    memset(&ifr, 0, sizeof(ifr));
    strncpy(ifr.ifr_name, name, sizeof(ifr.ifr_name) - 1);
    ifr.ifr_flags = IFF_LOOPBACK | IFF_UP;

    if ((ret = ioctl(fd, SIOCGIFFLAGS, &ifr)) < 0) {
        goto end;
    }
end:
    close(fd);
    return ret;
}

int net_create_tap(char *name) {
    struct ifreq ifr;
    int fd, ret;

    if ((fd = open("/dev/net/tun", O_RDWR)) < 0) {
        return fd;
    }

    memset(&ifr, 0, sizeof(ifr));
    ifr.ifr_flags = IFF_TAP | IFF_NO_PI;

    if (*name) {
        strncpy(ifr.ifr_name, name, sizeof(ifr.ifr_name) - 1);
    }

    if ((ret = ioctl(fd, TUNSETIFF, &ifr)) < 0) {
        goto err;
    }

    strcpy(name, ifr.ifr_name);
    return fd;
err:
    close(fd);
    return ret;
}

int net_create_unix_socket(char *path, char conn) {
    int fd, ret;

    struct sockaddr_un sa = {AF_UNIX, ""};
    strcpy(sa.sun_path, path);

    if ((fd = socket(AF_UNIX, SOCK_DGRAM, 0)) < 0) {
        fprintf(stdout, "failed to create socket\n");
        return fd;
    }

    if ((ret = fcntl(fd, F_SETFL, fcntl(fd, F_GETFL, 0) | O_NONBLOCK)) < 0){
        fprintf(stdout, "failed to fcntl O_NONBLOCK socket\n");
        return fd;
    }

    unlink(path);
    if ((ret = bind(fd, (struct sockaddr *) &sa, sizeof(sa))) < 0) {
        fprintf(stdout, "failed to bind socket\n");
        goto err;
    }

    if ((ret = chmod(path, S_ISUID | S_ISGID | S_ISVTX | S_IRWXU | S_IRWXG | S_IRWXO)) < 0) {
        fprintf(stdout, "failed to chmod socket\n");
        goto err;
    }

    if (conn) {
        if ((ret = connect(fd, (struct sockaddr *) &sa, sizeof(sa))) < 0) {
            fprintf(stdout, "failed to connect to socket\n");
            goto err;
        }
    }

//    if ((ret = listen(fd, 100)) < 0) {
//        fprintf(stdout, "failed to listen on socket\n");
//        goto err;
//    }

    return fd;
err:
    close(fd);
    return -1;
}

int net_connect_unix_socket(char *path) {
    int fd, ret;

    struct sockaddr_un sa = {AF_UNIX, ""};
    strcpy(sa.sun_path, path);

    if ((fd = socket(AF_UNIX, SOCK_DGRAM, 0)) < 0) {
        fprintf(stdout, "failed to create socket\n");
        return fd;
    }

    if ((ret = connect(fd, (struct sockaddr *) &sa, sizeof(sa))) < 0) {
        fprintf(stdout, "failed to connect to socket: %d\n", ret);
        goto err;
    }

    return fd;
err:
    close(fd);
    return -1;
}

int net_if_up(const char *name, int up) {
    struct ifreq ifr;
    int fd, ret;

    if ((fd = socket(PF_INET, SOCK_DGRAM, IPPROTO_IP)) < 0) {
        return fd;
    }

    memset(&ifr, 0, sizeof(ifr));
    strncpy(ifr.ifr_name, name, sizeof(ifr.ifr_name) - 1);

    if (up) {
        ifr.ifr_flags |= IFF_UP;
    } else {
        ifr.ifr_flags &= ~IFF_UP;
    }

    if ((ret = ioctl(fd, SIOCSIFFLAGS, &ifr)) < 0) {
        goto end;
    }
end:
    close(fd);
    return ret;
}

int net_if_mtu(const char *name, int mtu) {
    struct ifreq ifr;
    int fd, ret;

    if ((fd = socket(PF_INET, SOCK_DGRAM, IPPROTO_IP)) < 0) {
        return fd;
    }

    memset(&ifr, 0, sizeof(ifr));
    strncpy(ifr.ifr_name, name, sizeof(ifr.ifr_name) - 1);

    ifr.ifr_addr.sa_family = AF_INET;
    ifr.ifr_mtu = mtu;
    if ((ret = ioctl(fd, SIOCSIFMTU, &ifr)) < 0) {
        goto end;
    }
end:
    close(fd);
    return ret;
}

int net_if_addr(const char *name, const char *ip, const char *mask) {
    struct ifreq ifr;
    int fd, ret;

    if ((fd = socket(PF_INET, SOCK_DGRAM, IPPROTO_IP)) < 0) {
        return fd;
    }

    memset(&ifr, 0, sizeof(ifr));
    strncpy(ifr.ifr_name, name, sizeof(ifr.ifr_name) - 1);

    if ((ret = ioctl(fd, SIOCGIFADDR, &ifr)) == 0) {
        if ((ret = net_if_alias(&ifr, name)) < 0) {
            goto end;
        }
    }

    struct sockaddr_in* sa = (struct sockaddr_in*) &ifr.ifr_addr;
    sa->sin_family = AF_INET;

    if ((ret = inet_pton(AF_INET, ip, &sa->sin_addr)) < 0) {
        goto end;
    }
    if ((ret = ioctl(fd, SIOCSIFADDR, &ifr)) < 0) {
        goto end;
    }
    if ((ret = inet_pton(AF_INET, mask, &sa->sin_addr)) < 0) {
        goto end;
    }
    if ((ret = ioctl(fd, SIOCSIFNETMASK, &ifr)) < 0) {
        goto end;
    }

    ifr.ifr_flags = IFF_UP;
    if ((ret = ioctl(fd, SIOCSIFFLAGS, &ifr)) < 0) {
        goto end;
    }

end:
    close(fd);
    return ret;
}

int net_if_addr6(const char *name, const char *ip6) {
    struct ifreq ifr;
    struct ifreq6_stub ifr6;
    int fd, ret, pl;

    if ((fd = socket(PF_INET6, SOCK_DGRAM, IPPROTO_IP)) < 0) {
        return fd;
    }

    memset(&ifr, 0, sizeof(ifr));
    memset(&ifr6, 0, sizeof(ifr6));

    strncpy(ifr.ifr_name, name, sizeof(ifr.ifr_name) - 1);
    if ((ret = ioctl(fd, SIOGIFINDEX, &ifr)) < 0) {
        goto end;
    }

    if ((ret = ioctl(fd, SIOCGIFADDR, &ifr)) == 0) {
        if ((ret = net_if_alias(&ifr, name)) < 0) {
            goto end;
        }
    }

    if ((pl = parse_prefix_len(ip6)) < 0) {
        pl = 128;
    }

    ifr6.ifindex = ifr.ifr_ifindex;
    ifr6.prefixlen = pl;

    if ((ret = inet_pton(AF_INET6, ip6, (void *) &ifr6.addr)) < 0) {
        goto end;
    }
    if ((ret = ioctl(fd, SIOCSIFADDR, &ifr6)) < 0) {
        goto end;
    }

    ifr.ifr_flags |= IFF_UP;
    if ((ret = ioctl(fd, SIOCSIFFLAGS, &ifr)) < 0) {
        goto end;
    }

    if ((ret = net_if_mtu(ifr.ifr_name, MTU)) < 0) {
        goto end;
    }
end:
    close(fd);
    return ret;
}

int net_if_hw_addr(const char *name, const char mac[6]) {
    struct ifreq ifr;
    int fd, ret = 0;

    if ((fd = socket(AF_PACKET, SOCK_RAW, htons(ETH_P_ALL))) < 0) {
        return fd;
    }

    ifr.ifr_hwaddr.sa_family = ARPHRD_ETHER;
    memcpy(ifr.ifr_hwaddr.sa_data, mac, 6);

    strncpy(ifr.ifr_name, name, sizeof(ifr.ifr_name) - 1);
    if ((ret = ioctl(fd, SIOCSIFHWADDR, &ifr)) < 0) {
        goto err;
    }

err:
    close(fd);
    return ret;
}

int net_route(const char *name, const char *ip, const char *mask, const char *via) {
    struct rtentry rt;
    struct sockaddr_in *addr;
    int fd, ret = 0;

    fprintf(stdout, "net route\n");
    fflush(stdout);

    if ((fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_IP)) < 0) {
        return -1;
    }

    memset(&rt, 0, sizeof(rt));

    rt.rt_flags |= RTF_UP | RTF_GATEWAY;
    rt.rt_dev = malloc(strlen(name) + 1);
    if (!rt.rt_dev) {
        ret = -ENOMEM;
        goto end;
    }
    memcpy(rt.rt_dev, name, strlen(name) + 1);

    fprintf(stdout, "net route for %s\n", rt.rt_dev);
    fflush(stdout);

    addr = (struct sockaddr_in *) &rt.rt_gateway;
    addr->sin_family = AF_INET;
    addr->sin_addr.s_addr = inet_addr(via);

    addr = (struct sockaddr_in*) &rt.rt_dst;
    addr->sin_family = AF_INET;

    if (!ip) {
        addr->sin_addr.s_addr = INADDR_ANY;
        rt.rt_metric = 0;
    } else {
        addr->sin_addr.s_addr = inet_addr(ip);
        rt.rt_metric = 101;
    }

    addr = (struct sockaddr_in *) &rt.rt_genmask;
    addr->sin_family = AF_INET;

    fprintf(stdout, "net mask for %s\n", rt.rt_dev);
    fflush(stdout);

    if (!mask) {
        addr->sin_addr.s_addr = INADDR_ANY;
    } else {
        addr->sin_addr.s_addr = inet_addr(mask);
    }

    fprintf(stdout, "ioctl for %s\n", rt.rt_dev);
    fflush(stdout);

    if ((ret = ioctl(fd, SIOCADDRT, (void *) &rt)) < 0) {
        fprintf(stdout, "ioctl failed %d\n", ret);
        fflush(stdout);
        goto end;
    }

    fprintf(stdout, "done for %s\n", rt.rt_dev);
    fflush(stdout);

end:
    if (rt.rt_dev) free(rt.rt_dev);
    close(fd);
    return ret;
}

int net_route6(const char *name, const char *ip6, const char *via) {
    struct ifreq ifr;
    struct in6_rtmsg rt;
    int fd, pl, ret = 0;

    if ((fd = socket(AF_INET6, SOCK_DGRAM, 0)) < 0) {
        return -1;
    }
    strncpy(ifr.ifr_name, name, sizeof(ifr.ifr_name) - 1);
    if ((ret = ioctl(fd, SIOGIFINDEX, &ifr)) < 0) {
        goto end;
    }

    memset(&rt, 0, sizeof(rt));

    if (!ip6) {
        ip6 = "0:0:0:0:0:0:0:0";
    }

    if ((pl = parse_prefix_len(ip6)) < 0) {
        pl = 128;
    }

    rt.rtmsg_dst_len = pl;
    rt.rtmsg_metric = 101;
    rt.rtmsg_ifindex = ifr.ifr_ifindex;
    rt.rtmsg_flags |= RTF_UP | RTF_GATEWAY;

    if ((ret = inet_pton(AF_INET6, via, (void *) &(rt.rtmsg_gateway))) < 0) {
        goto end;
    }

    if ((ret = inet_pton(AF_INET6, ip6, (void *) &(rt.rtmsg_dst))) < 0) {
        goto end;
    }

    if ((ret = ioctl(fd, SIOCADDRT, (void *) &rt)) < 0) {
        goto end;
    }
end:
    close(fd);
    return ret;
}

union b_u16 {
    uint16_t i;
    char b[2];
};


int pump(
    int mtu,
    int tun_fd,
    int read_fd,
    int write_fd,
    char *read_sock,
    char *write_sock
) {

    int hsz = 2;
    mtu += hsz;

    char rbuf[mtu], wbuf[mtu];
    unsigned addr_sz = sizeof(struct sockaddr_un);
    memset(&rbuf, 0, sizeof(rbuf));
    memset(&wbuf, 0, sizeof(wbuf));

    struct sockaddr_un read_un = {AF_UNIX, ""};
    struct sockaddr_un write_un = {AF_UNIX, ""};

    strcpy(read_un.sun_path, read_sock);
    strcpy(write_un.sun_path, write_sock);

    int roff = 0, woff = 0;
    int rtotal = 0, wtotal = 0;
    int count = 0;

    while (1) {
        fd_set readset;
        FD_ZERO(&readset);
        FD_SET(tun_fd, &readset);
        FD_SET(read_fd, &readset);

        int max_fd = (tun_fd > read_fd ? tun_fd : read_fd) + 1;
        if (select(max_fd, &readset, NULL, NULL, NULL) < 0) {
            return errno;
        }

        if (FD_ISSET(tun_fd, &readset)) {
            if (rtotal == 0) {
                count = read(tun_fd, rbuf + hsz, mtu);
                if (count < 0) {
                    if (errno == EAGAIN) {
                        continue;
                    } else {
                        return errno;
                    }
                } else if (count == 0) {
                    continue;
                }

                union b_u16 *view = (union b_u16*)(void *) rbuf;
                view->i = count;

                rtotal = count + hsz;
                roff = 0;
            }

            count = sendto(write_fd, rbuf + roff, rtotal - roff, 0, (const struct sockaddr *)&write_un, addr_sz);
            if (count < 0) {
                if (errno == EAGAIN) {
                    continue;
                } else {
                    return errno;
                }
            }
            roff += count;

            if (roff >= rtotal) {
                roff = 0;
                rtotal = 0;
            }
        }

        if (FD_ISSET(read_fd, &readset)) {
            if (wtotal == 0) {
                count = recvfrom(read_fd, wbuf, mtu, 0, (struct sockaddr *)&read_un, &addr_sz);
                if (count < 0) {
                    if (errno == EAGAIN) {
                        continue;
                    } else {
                        return errno;
                    }
                } else if (count == 0) {
                    continue;
                }

                union b_u16 *view = (union b_u16*)(void *) wbuf;
                wtotal = view->i;
            }

            if (woff < wtotal) {
                count = write(tun_fd, wbuf + woff + hsz, wtotal - woff);
                if (count < 0) {
                    if (errno == EAGAIN) {
                        continue;
                    } else {
                        return errno;
                    }
                }
                woff += count;
            }

            if (woff >= wtotal) {
                woff = 0;
                wtotal = 0;
            }
        }
    }

    return 0;
}
