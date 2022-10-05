FROM alpine:latest

ENV SSHPASS=pass
ENV IP_ADDR="10.0.0.2"
ENV IP_GW="10.0.0.1"
ENV TAP_NAME="tap0"

VOLUME /golem/output

RUN apk add --no-cache --update file bash openssh iproute2 net-tools socat libnl3-dev readline tcpdump binutils zlib perf

# Set root password
RUN echo -e "$SSHPASS\n$SSHPASS" | passwd

# Generate ssh key
RUN mkdir /root/.ssh && \
    chmod 700 /root/.ssh && \
    ssh-keygen -A

RUN ssh-keygen -q -t rsa -P '' -f /root/.ssh/id_rsa && \
    chmod 600 /root/.ssh/id_rsa && \
    chmod 600 /root/.ssh/id_rsa.pub && \
    cat /root/.ssh/id_rsa.pub >> /root/.ssh/authorized_keys && \
    chmod 600 /root/.ssh/authorized_keys

RUN echo "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQDy0ZDXtYs4p8y20i+qldfbi4q8bGRkOPsTuUOTfXTHCnoFp4//CSy1vJBPkiC8K3GuwXn5AGPzzmJzJJip9fd4VrScXTnakiMoqOzEt4+AMOrjDrTbhFfidw9zXblQLheqy4z5R4HONqAsLao3Etzo7ZWDvg0tHCCiNIDVQsRySserbHj9OOrmN3wQUXV4ScG2/rtNL66HnwT5c+iz499JexgzVHnhP8yk8Nt5csxmPc4326PZYCPLISrHWLCPgHnXZOxJU0EwnYMwa++jwchYkWrjA6B6GaF1Q5BWr/s7nDw8Gun2PWMtyDpLzH2d+J7hVEDqy1aFL6WCAnWjUyAT4M/sXFzX2INTj07qt8/jasgd/3uXLM4UsF83I95Anec+hIOYI3K9v8XaRh/j3dhCPbSdAILRr9MMf9lMoU7HmBnt4I5OWuwZDhkAhGDl4DuLwQTnhrHkOWpN3NCo/er91M+BL0wDyvg1sY9l8wlZd0BTwhGhunNbKg6Q5DjP2H0= root@8071de022ddb" >> /root/.ssh/authorized_keys

# Setup config
RUN echo "Host *" >> /root/.ssh/config && \
    echo "  StrictHostKeyChecking=no" >> /root/.ssh/config

# Enable logging to root with password
RUN echo "UseDNS no" >> /etc/ssh/sshd_config && \
    echo "UsePAM yes" && \
    echo "PermitRootLogin yes" >> /etc/ssh/sshd_config && \
    echo "PasswordAuthentication yes" >> /etc/ssh/sshd_config

ADD entrypoint.sh /
ADD pump/pump /

RUN chmod 777 -R /golem

ENTRYPOINT ["/bin/bash", "/entrypoint.sh"]
