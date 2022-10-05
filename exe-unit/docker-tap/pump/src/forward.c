#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/socket.h>
#include <net/if.h>
#include <netinet/in.h>
#include <sched.h>
#include <signal.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <sys/un.h>
#include <threads.h>
#include <unistd.h>

typedef struct cpu_set_t { unsigned long __bits[128/sizeof(long)]; } cpu_set_t;

#include <liburing.h>
#include "forward.h"

#define QUEUE_DEPTH         8

static int working = true;

struct fwd_args {
    int *fds;
    uint16_t read_sz;
    bool read_hdr;
    bool write_hdr;
};

union b_u16 {
    uint16_t i;
    char b[2];
};

int fwd(void *data);

struct fwd_args* fwd_build_args(
    int rfd,
    int wfd,
    uint16_t read_sz,
    char read_hdr,
    char write_hdr
) {
    int *fds = 0;
    struct fwd_args *args = 0;

    if (!(fds = malloc(2 * sizeof(int)))) {
        goto err;
    }
    if (!(args = malloc(sizeof(struct fwd_args)))) {
        goto err;
    }

    fds[0] = rfd;
    fds[1] = wfd;
    args->fds = fds;
    args->read_sz = read_sz;
    args->read_hdr = read_hdr;
    args->write_hdr = write_hdr;

    return args;

err:
    if (fds) free(fds);
    if (args) free(args);

    return NULL;
}

int fwd_start_inplace(
    int rfd,
    int wfd,
    uint16_t read_sz,
    char read_hdr,
    char write_hdr
) {
    struct fwd_args *args = 0;

    fprintf(stdout, "fwd_start_inplace %d -> %d\n", rfd, wfd);
    fflush(stdout);

    if ((args = fwd_build_args(rfd, wfd, read_sz, read_hdr, write_hdr)) == 0) {
        return -ENOMEM;
    }

    fprintf(stdout, "fwd_start_inplace read_sz %d\n", read_sz);
    fflush(stdout);

    fwd(args);

    return 0;
}

int fwd_start(
    int rfd,
    int wfd,
    uint16_t read_sz,
    char read_hdr,
    char write_hdr
) {
    thrd_t th;
    int ret;
    struct fwd_args *args;


    fprintf(stdout, "fwd_start %d -> %d\n", rfd, wfd);
    fflush(stdout);

    if ((args = fwd_build_args(rfd, wfd, read_sz, read_hdr, write_hdr)) == 0) {
        return -ENOMEM;
    }

    fprintf(stdout, "read_sz %d\n", read_sz);
    fflush(stdout);

    if ((ret = thrd_create(&th, fwd, (void*) args)) != thrd_success) {
        fprintf(stdout, "FAILED TO CREATE THREAD %d\n", read_sz);
        fflush(stdout);

        goto err;
    }
    return thrd_detach(th);

err:
    if (args) {
        free(args->fds);
        free(args);
    }
    return ret;
}

void fwd_stop() {
    working = false;
}

int read_fd(
    struct io_uring *ring,
    int fd,
    int real_fd,
    char *dst,
    uint16_t count,
    char exact
) {
    struct io_uring_sqe *sqe;
    struct io_uring_cqe *cqe;

    int ret = 0;
    unsigned rem = 0;
    uint16_t rc = 0;
    uint16_t ro = 0;

    while (working && ro < count) {
        if (!(sqe = io_uring_get_sqe(ring))) {
            fprintf(stdout, "READ %d io_uring_get_sqe\n", real_fd);
            fflush(stdout);

            return -1;
        }

        rem = count - ro;

//        fprintf(stdout, "TRY READ %d >> %d / %d B\n", real_fd, rem, count);
//        if (exact) {
//            io_uring_prep_read(sqe, real_fd, dst + ro, rem, 0);
//        } else {
            io_uring_prep_read(sqe, fd, dst + ro, rem, 0);
            sqe->flags |= IOSQE_FIXED_FILE;
//        }


//        io_uring_prep_read(sqe, fd, dst + ro, rem, 0);
//        sqe->flags |= IOSQE_FIXED_FILE;
//        sqe->buf_index = 0;
//
//        if (exact) {
//            sqe->opcode = IORING_OP_READ_FIXED;
//        }

        io_uring_submit(ring);

        if ((ret = io_uring_wait_cqe(ring, &cqe)) < 0) {
            fprintf(stdout, "READ %d io_uring_wait_cqe\n", real_fd);
            fflush(stdout);

            return ret;
        }

        if (cqe->res < 0) {
            fprintf(stderr, "CQE err %d (%s)\n", cqe->res, exact ? "recv" : "read");
            fflush(stderr);

            return 0;
        }

        rc = cqe->res;
        io_uring_cqe_seen(ring, cqe);
        if (rc <= 0) {
            continue;
        }

        fprintf(stdout, "READ %d (%d) >> %d out of %d B (%s)\n", real_fd, fd, rc, count, exact ? "recv" : "read");
        fflush(stdout);

//        for(int i = 0; i < rc; i++)
//            fprintf(stdout, "%d ", dst[ro + i]);
//        fprintf(stdout, "\n");

        ro += rc;



        if (!exact) {
            break;
        }
    }

    return ro;
}

//int readv_fd(
//    struct io_uring *ring,
//    int fd,
//    const struct iovec *iovecs,
//    uint16_t nr_vecs
//) {
//
//}

int write_fd(
    struct io_uring *ring,
    int fd,
    char *src,
    uint16_t count
) {
    struct io_uring_sqe *sqe;
    struct io_uring_cqe *cqe;

    int ret = 0;
    int wc = 0;
    size_t wo = 0;

    while (working && wo < count) {
        if (!(sqe = io_uring_get_sqe(ring))) {
            fprintf(stdout, "WRITE %d io_uring_get_sqe\n", fd);
            fflush(stdout);
            return -1;
        }

        io_uring_prep_write(sqe, fd, src + wo, count - wo, 0);
        sqe->flags |= IOSQE_FIXED_FILE;

        io_uring_submit(ring);
        if ((ret = io_uring_wait_cqe(ring, &cqe)) < 0) {
            fprintf(stdout, "WRITE %d io_uring_wait_cqe\n", fd);
            fflush(stdout);
            return ret;
        }

        wc = cqe->res;
        io_uring_cqe_seen(ring, cqe);
        if (wc < 0) {
            fprintf(stdout, "WRITE %d err %d\n", fd, wc);
            return -2;
        }
        fprintf(stdout, "WRITE %d B\n", wc);
        fflush(stdout);
        wo += wc;
    }

    return 0;
}

int writev_fd(
    struct io_uring *ring,
    int fd,
    const struct iovec *iovecs,
    uint16_t nr_vecs
) {
    struct io_uring_sqe *sqe;
    struct io_uring_cqe *cqe;

    int ret = 0;
    int wc = 0;
    size_t count = 0;
    size_t wo = 0;

    for (size_t i = 0; i < nr_vecs; ++i) {
        count = iovecs[i].iov_len;
    }

    while (working && wo < count) {
        if (!(sqe = io_uring_get_sqe(ring))) {
            fprintf(stdout, "WRITE %d io_uring_get_sqe\n", fd);
            fflush(stdout);
            return -1;
        }

        io_uring_prep_writev(sqe, fd, iovecs, nr_vecs, 0);
        sqe->flags |= IOSQE_FIXED_FILE;

        io_uring_submit(ring);
        if ((ret = io_uring_wait_cqe(ring, &cqe)) < 0) {
            fprintf(stdout, "WRITE %d io_uring_wait_cqe ERROR: %d\n", fd, ret);
            fflush(stdout);
            return ret;
        }

        wc = cqe->res;
        io_uring_cqe_seen(ring, cqe);
        if (wc < 0) {
            fprintf(stdout, "WRITE %d io_uring_cqe_seen ERROR: %d\n", fd, wc);
            fflush(stdout);
            return wc;
        }

        fprintf(stdout, "WRITE %d B\n", wc);
        fflush(stdout);
        wo += wc;
    }

    return 0;
}

int fwd(void *data) {
    struct io_uring ring;
//    struct io_uring_params params;

    struct fwd_args *args = (struct fwd_args*) data;

    union b_u16 sz;
    int  ret = 0, rfd = 0, wfd = 1;
    char exact = 0;
    char *buf = 0;

    fprintf(stdout, "forward %d -> %d\n", args->fds[0], args->fds[1]);
    fflush(stdout);

    if (!(buf = malloc(args->read_sz))) {
        ret = -ENOMEM;
        goto end;
    }

//    memset(&params, 0, sizeof(params));
//    params.flags |= IORING_SETUP_IOPOLL;
//    //params.sq_thread_idle = 2000;

    io_uring_queue_init(QUEUE_DEPTH, &ring, 0);
//    io_uring_queue_init_params(QUEUE_DEPTH, &ring, &params);

    if ((ret = io_uring_register_files(&ring, args->fds, 2)) < 0) {
        goto end;
    }

//    if (args->read_hdr) {
//        if ((ret = prep_read_socket(&ring, args->fds[0])) < 0) {
//            goto end;
//        }
//    }

    while (working) {
        fprintf(stdout, "working\n");
        fflush(stdout);

        if (args->read_hdr) {
            exact = 1;
            if ((ret = read_fd(&ring, rfd, args->fds[0], buf, 2, exact)) < 0) {
                fprintf(stdout, "! read_fd (1) failed %d\n", ret);
                fflush(stdout);
                goto end;
            }
            sz.b[0] = buf[0];
            sz.b[1] = buf[1];
        } else {
            exact = 0;

            sz.i = args->read_sz;
        }

        if ((ret = read_fd(&ring, rfd, args->fds[0], buf, sz.i, exact)) < 0) {
            fprintf(stdout, "! read_fd (2) failed %d\n", ret);
            fflush(stdout);
            goto end;
        }

        sz.i = ret;
        if (sz.i <= 0) {
            continue;
        }

        fprintf(stdout, "wfd %d going to write %d\n", args->fds[1], sz.i);
        fflush(stdout);

        if (args->write_hdr) {
            struct iovec iovecs[] = {
                { .iov_base = &sz.b, .iov_len = sizeof(sz.i) },
                { .iov_base = buf, .iov_len = (size_t) sz.i },
            };
            if ((ret = writev_fd(&ring,
                                 wfd,
                                 (struct iovec*) &iovecs,
                                 sizeof(iovecs) / sizeof(struct iovec))) < 0) {
                fprintf(stdout, "! writev_fd (1) failed %d\n", ret);
                fflush(stdout);
                goto end;
            }
        } else {
            if ((ret = write_fd(&ring, wfd, buf, sz.i)) < 0) {
                fprintf(stdout, "! write_fd (2) failed %d\n", ret);
                fflush(stdout);
                goto end;
            }
        }
    }

end:
    io_uring_unregister_files(&ring);
    free(args->fds);
    free(args);
    if (buf) free(buf);
    return ret;
}
