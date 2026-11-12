#include <sys/acl.h>
#include <stdlib.h>
#include <stdio.h>
#include <errno.h>
#include <unistd.h>
#include <limits.h>
_Static_assert(sizeof(acl_type_t) == sizeof(int), "acl_type_t ABI");
_Static_assert(sizeof(acl_tag_t) == sizeof(int), "acl_tag_t ABI");
_Static_assert(ACL_TYPE_EXTENDED == 0x100, "ACL_TYPE_EXTENDED");
_Static_assert(ACL_EXTENDED_ALLOW == 1, "ACL_EXTENDED_ALLOW");
_Static_assert(ACL_EXTENDED_DENY == 2, "ACL_EXTENDED_DENY");
_Static_assert(ACL_FIRST_ENTRY == 0, "ACL_FIRST_ENTRY");
_Static_assert(ACL_NEXT_ENTRY == -1, "ACL_NEXT_ENTRY");
int main(void) {
    acl_t (*fd_acquisition)(int, acl_type_t) = acl_get_fd_np;
    (void)fd_acquisition;
    char path[PATH_MAX];
    const char *parent = getenv("LARPCC_TEST_PARENT");
    if (!parent || snprintf(path, sizeof(path), "%s/acl-abi-XXXXXX", parent) >= (int)sizeof(path)) return 1;
    if (!mkdtemp(path)) return 2;
    acl_t acl = acl_get_link_np(path, ACL_TYPE_EXTENDED);
    if (!acl) { perror("native no-ACL acquisition"); rmdir(path); return 3; }
    if (acl_valid(acl) != 0) { acl_free(acl); rmdir(path); return 4; }
    acl_entry_t entry;
    errno = 0;
    int result = acl_get_entry(acl, ACL_FIRST_ENTRY, &entry);
    if (result != -1 || errno != EINVAL) {
        fprintf(stderr, "unexpected empty ACL iterator result=%d errno=%d\n", result, errno);
        acl_free(acl); rmdir(path); return 5;
    }
    acl_free(acl);
    if (rmdir(path) != 0) return 6;
    puts("Darwin SDK ABI, empty ACL allocation and -1/EINVAL iteration confirmed");
    return 0;
}
