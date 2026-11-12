#include <sys/acl.h>
_Static_assert(sizeof(acl_type_t) == sizeof(int), "acl_type_t ABI");
_Static_assert(sizeof(acl_tag_t) == sizeof(int), "acl_tag_t ABI");
_Static_assert(ACL_TYPE_EXTENDED == 0x100, "ACL_TYPE_EXTENDED");
_Static_assert(ACL_EXTENDED_ALLOW == 1, "ACL_EXTENDED_ALLOW");
_Static_assert(ACL_EXTENDED_DENY == 2, "ACL_EXTENDED_DENY");
_Static_assert(ACL_FIRST_ENTRY == 0, "ACL_FIRST_ENTRY");
_Static_assert(ACL_NEXT_ENTRY == -1, "ACL_NEXT_ENTRY");
int main(void) { return 0; }
