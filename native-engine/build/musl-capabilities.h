/* musl provides capget(2) and capset(2) but declares them in no header (its
 * users are expected to bring libcap's). strongSwan's native capability
 * backend (--with-capabilities=native, ADR-0028) calls them directly, so the
 * engine build includes these declarations instead of adding libcap. */
#ifndef IPSEC_CLIENT_MUSL_CAPABILITIES_H
#define IPSEC_CLIENT_MUSL_CAPABILITIES_H
#include <linux/capability.h>
int capget(cap_user_header_t hdrp, cap_user_data_t datap);
int capset(cap_user_header_t hdrp, const cap_user_data_t datap);
#endif
