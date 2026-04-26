package build

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestClassifyBuild_NonRHEL(t *testing.T) {
	cf := "FROM quay.io/fedora/fedora-bootc:43\nRUN dnf install -y vim\n"
	assert.Equal(t, DetectionNonEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_EntitledRHEL(t *testing.T) {
	cf := "FROM registry.redhat.io/rhel9/rhel-bootc:9.4\nRUN dnf install -y httpd\n"
	assert.Equal(t, DetectionEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_UBI(t *testing.T) {
	cf := "FROM registry.redhat.io/ubi9/ubi:latest\nRUN dnf install -y vim\n"
	assert.Equal(t, DetectionNonEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_UBIMinimal(t *testing.T) {
	cf := "FROM registry.redhat.io/ubi9/ubi-minimal:latest\nRUN microdnf install -y vim\n"
	assert.Equal(t, DetectionNonEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_UBITopLevel(t *testing.T) {
	cf := "FROM registry.redhat.io/ubi8:latest\n"
	assert.Equal(t, DetectionNonEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_MultiStageEntitled(t *testing.T) {
	cf := "FROM registry.redhat.io/ubi9/ubi AS builder\nRUN echo hi\nFROM registry.redhat.io/rhel9/rhel-bootc:9.4\nRUN dnf install -y httpd\n"
	assert.Equal(t, DetectionEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_PlatformFlag(t *testing.T) {
	cf := "FROM --platform=linux/arm64 registry.redhat.io/rhel9/rhel-bootc:9.4\n"
	assert.Equal(t, DetectionEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_ARGWithDefault(t *testing.T) {
	cf := "ARG BASE=registry.redhat.io/rhel9/rhel-bootc:9.4\nFROM ${BASE}\n"
	assert.Equal(t, DetectionAmbiguous, ClassifyBuild(cf))
}

func TestClassifyBuild_ARGWithUBIDefault(t *testing.T) {
	cf := "ARG BASE=registry.redhat.io/ubi9/ubi:latest\nFROM ${BASE}\n"
	assert.Equal(t, DetectionAmbiguous, ClassifyBuild(cf))
}

func TestClassifyBuild_ARGNoDefault(t *testing.T) {
	cf := "ARG BASE\nFROM ${BASE}\n"
	assert.Equal(t, DetectionAmbiguous, ClassifyBuild(cf))
}

func TestClassifyBuild_MixedPublicAndUnresolved(t *testing.T) {
	cf := "FROM quay.io/fedora/fedora:43 AS builder\nARG PROD\nFROM ${PROD}\n"
	assert.Equal(t, DetectionAmbiguous, ClassifyBuild(cf))
}

func TestClassifyBuild_CommentsAndBlanks(t *testing.T) {
	cf := "# This is a comment\n\nFROM registry.redhat.io/ubi9/ubi:latest\n"
	assert.Equal(t, DetectionNonEntitled, ClassifyBuild(cf))
}
