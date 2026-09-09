library(stats)

ModelConfig <- setRefClass("ModelConfig")

calculate_score <- function(x) {
  s <- sum(x)
  return(format_output(s))
}

format_output <- function(val) {
  as.character(val)
}
