#include <sys/acl.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <membership.h>
#include <stdlib.h>
#include <stdio.h>
#include <errno.h>
#include <unistd.h>
#include <limits.h>
_Static_assert(sizeof(acl_type_t) == sizeof(int), "acl_type_t ABI");
_Static_assert(sizeof(acl_tag_t) == sizeof(int), "acl_tag_t ABI");
_Static_assert(sizeof(filesec_property_t) == sizeof(int), "filesec_property_t ABI");
_Static_assert(FILESEC_ACL == 5, "FILESEC_ACL");
_Static_assert(_PC_EXTENDED_SECURITY_NP == 13, "extended security pathconf");
_Static_assert(ACL_TYPE_EXTENDED == 0x100, "ACL_TYPE_EXTENDED");
_Static_assert(ACL_EXTENDED_ALLOW == 1, "ACL_EXTENDED_ALLOW");
_Static_assert(ACL_EXTENDED_DENY == 2, "ACL_EXTENDED_DENY");
_Static_assert(ACL_FIRST_ENTRY == 0, "ACL_FIRST_ENTRY");
_Static_assert(ACL_NEXT_ENTRY == -1, "ACL_NEXT_ENTRY");
static void require(int condition, const char *label) {
    if (!condition) { perror(label); exit(1); }
}
static int scan(acl_t acl, int expected_tag) {
    require(acl_valid(acl) == 0, "acl_valid");
    acl_entry_t entry = NULL;
    int which = ACL_FIRST_ENTRY, count = 0;
    for (;;) {
        errno = 0;
        int result = acl_get_entry(acl, which, &entry);
        if (result == -1) {
            require(errno == EINVAL, "iterator exhaustion errno");
            break;
        }
        require(result == 0 && entry != NULL, "iterator entry");
        acl_tag_t tag = ACL_UNDEFINED_TAG;
        require(acl_get_tag_type(entry, &tag) == 0 && tag == expected_tag, "iterator tag");
        count++;
        which = ACL_NEXT_ENTRY;
    }
    return count;
}
static int presence(int fd, int expected_tag) {
    struct stat ordinary = {0}, extended = {0};
    require(fstat(fd, &ordinary) == 0, "valid descriptor fstat");
    require(fpathconf(fd, _PC_EXTENDED_SECURITY_NP) == 1, "volume extended-security support");
    filesec_t security = filesec_init();
    require(security != NULL, "filesec_init");
    require(fstatx_np(fd, &extended, security) == 0, "descriptor extended-security query");
    require(ordinary.st_dev == extended.st_dev && ordinary.st_ino == extended.st_ino, "descriptor identity");
    int present = -1;
    require(filesec_query_property(security, FILESEC_ACL, &present) == 0, "ACL property presence");
    if (present) {
        acl_t acl = NULL;
        require(filesec_get_property(security, FILESEC_ACL, &acl) == 0 && acl != NULL, "ACL property copy");
        int count = scan(acl, expected_tag);
        require(count == (expected_tag ? 1 : 0), "ACL entry count");
        require(acl_free(acl) == 0, "copied ACL free");
    } else {
        acl_t absent = NULL;
        errno = 0;
        require(filesec_get_property(security, FILESEC_ACL, &absent) == -1 && errno == ENOENT, "absent property errno");
        require(expected_tag == 0, "real ACL must be present");
    }
    filesec_free(security);
    return present != 0;
}
int main(void) {
    /* Assignments validate the SDK declarations without guessing symbol aliases. */
    acl_t (*fd_acquisition)(int, acl_type_t) = acl_get_fd_np;
    int (*extended_stat)(int, struct stat *, filesec_t) = fstatx_np;
    (void)fd_acquisition; (void)extended_stat;
    char path[PATH_MAX];
    const char *parent = getenv("LARPCC_TEST_PARENT");
    require(parent && snprintf(path, sizeof(path), "%s/acl-abi-XXXXXX", parent) < (int)sizeof(path), "fixture parent");
    require(mkdtemp(path) != NULL, "private fixture creation");
    int fd = open(path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    require(fd >= 0, "fixture descriptor");
    require(presence(fd, 0) == 0, "fresh directory ACL absence");
    errno = 0;
    acl_t raw = acl_get_fd_np(fd, ACL_TYPE_EXTENDED);
    require(raw == NULL && errno == ENOENT, "valid-fd ACL absence errno");
    errno = 0;
    raw = acl_get_link_np(path, ACL_TYPE_EXTENDED);
    require(raw == NULL && errno == ENOENT, "valid-path ACL absence errno");
    puts("valid descriptor: extended-security supported; fstatx success; ACL property absent; wrapper ENOENT");
    filesec_t bad = filesec_init();
    require(bad != NULL, "failure fixture allocation");
    struct stat unused = {0};
    errno = 0;
    require(fstatx_np(-1, &unused, bad) == -1 && errno == EBADF, "invalid fd query must fail");
    int present = -1;
    errno = 0;
    require(filesec_query_property(bad, (filesec_property_t)-1, &present) == -1 && errno == EINVAL, "invalid property must fail");
    filesec_free(bad);
    char missing[PATH_MAX];
    require(snprintf(missing, sizeof(missing), "%s/missing", path) < (int)sizeof(missing), "missing path length");
    errno = 0;
    require(acl_get_link_np(missing, ACL_TYPE_EXTENDED) == NULL && errno == ENOENT, "missing path differs from valid-fd absence");
    acl_t empty = acl_init(0);
    require(empty != NULL && scan(empty, 0) == 0, "allocated empty ACL iteration");
    require(acl_set_fd_np(fd, empty, ACL_TYPE_EXTENDED) == 0, "fixture empty ACL");
    printf("real empty ACL: property_present=%d; iteration exhaustion=-1/EINVAL\n", presence(fd, 0));
    uuid_t user;
    require(mbr_uid_to_uuid(getuid(), user) == 0, "synthetic ACL qualifier");
    for (int tag = ACL_EXTENDED_ALLOW; tag <= ACL_EXTENDED_DENY; tag++) {
        acl_t acl = acl_init(1);
        acl_entry_t entry = NULL;
        acl_permset_t permissions = NULL;
        require(acl && acl_create_entry(&acl, &entry) == 0, "synthetic ACL entry");
        require(acl_set_tag_type(entry, tag) == 0 && acl_set_qualifier(entry, user) == 0, "synthetic ACL tag/qualifier");
        require(acl_get_permset(entry, &permissions) == 0 && acl_add_perm(permissions, ACL_READ_DATA) == 0, "synthetic ACL permission");
        require(acl_set_fd_np(fd, acl, ACL_TYPE_EXTENDED) == 0, "synthetic allow/deny ACL attach");
        require(presence(fd, tag) == 1, "real allow/deny ACL presence");
        require(acl_free(acl) == 0, "synthetic ACL free");
    }
    require(acl_set_fd_np(fd, empty, ACL_TYPE_EXTENDED) == 0, "synthetic ACL cleanup");
    require(acl_free(empty) == 0, "empty ACL free");
    require(close(fd) == 0 && rmdir(path) == 0, "fixture cleanup");
    puts("Darwin SDK ABI, valid-fd absence, query errors and empty/allow/deny iterator conventions confirmed");
    return 0;
}
