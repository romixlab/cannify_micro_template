// GitHub's social preview of the repo (Settings > Social preview takes a PNG); `just logo` renders it.
#import "template_logo.typ": social-preview
#import "logo.typ": values

#show: social-preview.with(..values)
