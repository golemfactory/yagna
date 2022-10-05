#ifndef _NETWORK_H
#define _NETWORK_H

#define MTU 1486

int net_create_lo(char *name);
int net_create_tap(char *name);
int net_create_unix_socket(char *path, char conn);
int net_connect_unix_socket(char *path);

int net_if_up(const char *name, int up);
int net_if_mtu(const char *name, int mtu);
int net_if_addr(const char *name, const char *ip, const char *mask);
int net_if_addr6(const char *name, const char *ip6);
int net_if_hw_addr(const char *name, const char mac[6]);

int net_route(const char *name, const char *ip, const char *mask, const char *via);
int net_route6(const char *name, const char *ip6, const char *via);

int create_chardev(char *path);

int pump(
    int mtu,
    int tun_fd,
    int read_fd,
    int write_fd,
    char *read_sock,
    char *write_sock
);

#endif // _NETWORK_H
