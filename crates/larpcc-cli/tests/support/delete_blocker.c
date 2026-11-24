#include <unistd.h>

int main(void) {
    char byte;
    if (read(STDIN_FILENO, &byte, 1) != 1 ||
        write(STDOUT_FILENO, &byte, 1) != 1) {
        return 1;
    }
    while (read(STDIN_FILENO, &byte, 1) == 1) {}
    return 0;
}
